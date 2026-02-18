mod tts;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use eframe::egui;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use std::fs;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

fn strip_markdown(md: &str) -> String {
    use pulldown_cmark::{Event, Parser, TagEnd};
    let parser = Parser::new(md);
    let mut text = String::new();
    for event in parser {
        match event {
            Event::Text(t) | Event::Code(t) => text.push_str(&t),
            Event::SoftBreak | Event::HardBreak => text.push('\n'),
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_)) => text.push('\n'),
            _ => {}
        }
    }
    text
}

fn whisper_model_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".local/share/downbad")
        .join("ggml-base.en.bin")
}

fn transcribe_audio(
    ctx: Arc<WhisperContext>,
    samples: Vec<f32>,
    source_rate: u32,
) -> Result<String, String> {
    // Resample to 16kHz mono via linear interpolation
    let target_rate = 16000u32;
    let resampled = if source_rate == target_rate {
        samples
    } else {
        let ratio = source_rate as f64 / target_rate as f64;
        let output_len = (samples.len() as f64 / ratio) as usize;
        let mut output = Vec::with_capacity(output_len);
        for i in 0..output_len {
            let src_idx = i as f64 * ratio;
            let idx0 = src_idx as usize;
            let frac = (src_idx - idx0 as f64) as f32;
            let s0 = samples.get(idx0).copied().unwrap_or(0.0);
            let s1 = samples.get(idx0 + 1).copied().unwrap_or(s0);
            output.push(s0 + frac * (s1 - s0));
        }
        output
    };

    let mut state = ctx.create_state().map_err(|e| format!("Whisper state error: {e}"))?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_n_threads(4);
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    params.set_language(Some("en"));

    state
        .full(params, &resampled)
        .map_err(|e| format!("Whisper inference error: {e}"))?;

    let num_segments = state.full_n_segments();
    let mut text = String::new();
    for i in 0..num_segments {
        if let Some(segment) = state.get_segment(i) {
            if let Ok(s) = segment.to_str() {
                text.push_str(s);
            }
        }
    }
    Ok(text.trim().to_string())
}

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: db <filename>");
        std::process::exit(1);
    }

    let path = PathBuf::from(&args[1]);
    let path = fs::canonicalize(&path).unwrap_or(path);
    let content = if path.exists() {
        fs::read_to_string(&path).unwrap_or_else(|e| {
            eprintln!("Error reading {}: {e}", path.display());
            std::process::exit(1);
        })
    } else {
        String::new()
    };

    // Re-launch as a detached process so the shell prompt returns immediately.
    // Using Command instead of fork() avoids macOS window server issues with
    // forked processes (black windows, broken rendering).
    if std::env::var_os("DB_NO_FORK").is_none() {
        let exe = std::env::current_exe().expect("cannot find own executable");
        std::process::Command::new(&exe)
            .args(&args[1..])
            .env("DB_NO_FORK", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("failed to spawn detached process");
        std::process::exit(0);
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };

    eframe::run_native(
        &format!("db - {}", path.display()),
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(path, content)))),
    )
}

struct TtsPlayback {
    samples: Vec<f32>,
    cursor: usize,
    done: bool,
}

struct App {
    path: PathBuf,
    content: String,
    saved_content: String,
    dirty: bool,
    prev_dirty: bool,
    preview_mode: bool,
    prev_preview_mode: bool,
    commonmark_cache: CommonMarkCache,
    first_frame: bool,
    show_exit_dialog: bool,
    show_reload_dialog: bool,
    force_exit: bool,
    cursor_line: usize,
    cursor_col: usize,
    // Speech-to-text fields
    recording: bool,
    prev_recording: bool,
    audio_stream: Option<cpal::Stream>,
    audio_buffer: Arc<Mutex<Vec<f32>>>,
    audio_sample_rate: u32,
    audio_channels: u16,
    transcribing: bool,
    prev_transcribing: bool,
    transcription_rx: Option<mpsc::Receiver<Result<String, String>>>,
    whisper_ctx: Option<Arc<WhisperContext>>,
    stt_error: Option<String>,
    stt_error_time: Option<std::time::Instant>,
    // Text-to-speech fields
    speaking: bool,
    prev_speaking: bool,
    generating_speech: bool,
    prev_generating_speech: bool,
    tts_ctx: Option<Arc<tts::KokoroTts>>,
    tts_audio_stream: Option<cpal::Stream>,
    tts_playback: Option<Arc<Mutex<TtsPlayback>>>,
    tts_device_rate: u32,
    tts_device_channels: usize,
    speech_rx: Option<mpsc::Receiver<Result<Vec<f32>, String>>>,
}

impl App {
    fn new(path: PathBuf, content: String) -> Self {
        Self {
            path,
            saved_content: content.clone(),
            content,
            dirty: false,
            prev_dirty: false,
            preview_mode: false,
            prev_preview_mode: false,
            commonmark_cache: CommonMarkCache::default(),
            first_frame: true,
            show_exit_dialog: false,
            show_reload_dialog: false,
            force_exit: false,
            cursor_line: 0,
            cursor_col: 0,
            recording: false,
            prev_recording: false,
            audio_stream: None,
            audio_buffer: Arc::new(Mutex::new(Vec::new())),
            audio_sample_rate: 44100,
            audio_channels: 1,
            transcribing: false,
            prev_transcribing: false,
            transcription_rx: None,
            whisper_ctx: None,
            stt_error: None,
            stt_error_time: None,
            speaking: false,
            prev_speaking: false,
            generating_speech: false,
            prev_generating_speech: false,
            tts_ctx: None,
            tts_audio_stream: None,
            tts_playback: None,
            tts_device_rate: 0,
            tts_device_channels: 0,
            speech_rx: None,
        }
    }

    fn save(&mut self) {
        if let Err(e) = fs::write(&self.path, &self.content) {
            eprintln!("Error saving {}: {e}", self.path.display());
        } else {
            self.saved_content = self.content.clone();
            self.dirty = false;
        }
    }

    fn reload_file(&mut self) {
        match fs::read_to_string(&self.path) {
            Ok(new_content) => {
                self.content = new_content;
                self.saved_content = self.content.clone();
            }
            Err(e) => {
                self.stt_error = Some(format!("Reload failed: {e}"));
                self.stt_error_time = Some(std::time::Instant::now());
            }
        }
    }

    fn load_whisper_model(&mut self) -> Result<Arc<WhisperContext>, String> {
        if let Some(ref ctx) = self.whisper_ctx {
            return Ok(Arc::clone(ctx));
        }
        let model_path = whisper_model_path();
        if !model_path.exists() {
            return Err(format!(
                "Whisper model not found at {}. Download it first.",
                model_path.display()
            ));
        }
        let ctx = WhisperContext::new_with_params(
            model_path.to_str().unwrap_or_default(),
            WhisperContextParameters::default(),
        )
        .map_err(|e| format!("Failed to load Whisper model: {e}"))?;
        let ctx = Arc::new(ctx);
        self.whisper_ctx = Some(Arc::clone(&ctx));
        Ok(ctx)
    }

    fn start_recording(&mut self) -> Result<(), String> {
        // Fail-fast: check model file exists before recording
        let model_path = whisper_model_path();
        if !model_path.exists() {
            return Err(format!(
                "Whisper model not found at {}",
                model_path.display()
            ));
        }

        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or("No microphone found")?;
        let config = device
            .default_input_config()
            .map_err(|e| format!("No input config: {e}"))?;

        self.audio_sample_rate = config.sample_rate().0;
        self.audio_channels = config.channels();

        let buffer = Arc::clone(&self.audio_buffer);
        buffer.lock().unwrap().clear();

        let channels = self.audio_channels as usize;
        let stream = device
            .build_input_stream(
                &config.into(),
                move |data: &[f32], _: &cpal::InputCallbackInfo| {
                    let mut buf = buffer.lock().unwrap();
                    // Downmix to mono
                    if channels == 1 {
                        buf.extend_from_slice(data);
                    } else {
                        for chunk in data.chunks(channels) {
                            let sum: f32 = chunk.iter().sum();
                            buf.push(sum / channels as f32);
                        }
                    }
                },
                move |err| {
                    eprintln!("Audio input error: {err}");
                },
                None,
            )
            .map_err(|e| format!("Failed to build input stream: {e}"))?;

        stream.play().map_err(|e| format!("Failed to start recording: {e}"))?;

        self.audio_stream = Some(stream);
        self.recording = true;
        Ok(())
    }

    fn load_tts_model(&mut self) -> Result<Arc<tts::KokoroTts>, String> {
        if let Some(ref ctx) = self.tts_ctx {
            return Ok(Arc::clone(ctx));
        }
        let ctx = tts::KokoroTts::new()?;
        let ctx = Arc::new(ctx);
        self.tts_ctx = Some(Arc::clone(&ctx));
        Ok(ctx)
    }

    fn stop_speaking(&mut self) {
        self.tts_audio_stream = None;
        self.tts_playback = None;
        self.speaking = false;
    }

    fn start_speaking(&mut self, text: String) {
        let tts_ctx = match self.load_tts_model() {
            Ok(ctx) => ctx,
            Err(e) => {
                self.stt_error = Some(e);
                self.stt_error_time = Some(std::time::Instant::now());
                return;
            }
        };

        // Resolve output device config once up front
        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(d) => d,
            None => {
                self.stt_error = Some("No audio output device found".to_string());
                self.stt_error_time = Some(std::time::Instant::now());
                return;
            }
        };
        let config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                self.stt_error = Some(format!("No output config: {e}"));
                self.stt_error_time = Some(std::time::Instant::now());
                return;
            }
        };
        self.tts_device_rate = config.sample_rate().0;
        self.tts_device_channels = config.channels() as usize;

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            tts_ctx.synthesize_streaming(&text, tx);
        });

        self.speech_rx = Some(rx);
        self.generating_speech = true;
    }

    fn start_audio_stream(&mut self, playback: Arc<Mutex<TtsPlayback>>) {
        let host = cpal::default_host();
        let device = match host.default_output_device() {
            Some(d) => d,
            None => {
                self.stt_error = Some("No audio output device found".to_string());
                self.stt_error_time = Some(std::time::Instant::now());
                return;
            }
        };
        let config = match device.default_output_config() {
            Ok(c) => c,
            Err(e) => {
                self.stt_error = Some(format!("No output config: {e}"));
                self.stt_error_time = Some(std::time::Instant::now());
                return;
            }
        };

        let device_channels = config.channels() as usize;

        let stream = match device.build_output_stream(
            &config.into(),
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let mut state = playback.lock().unwrap();
                for frame in data.chunks_mut(device_channels) {
                    let sample = if state.cursor < state.samples.len() {
                        let s = state.samples[state.cursor];
                        state.cursor += 1;
                        s
                    } else {
                        0.0
                    };
                    for ch in frame.iter_mut() {
                        *ch = sample;
                    }
                }
            },
            |err| {
                eprintln!("Audio output error: {err}");
            },
            None,
        ) {
            Ok(s) => s,
            Err(e) => {
                self.stt_error = Some(format!("Failed to build output stream: {e}"));
                self.stt_error_time = Some(std::time::Instant::now());
                return;
            }
        };

        if let Err(e) = stream.play() {
            self.stt_error = Some(format!("Failed to start playback: {e}"));
            self.stt_error_time = Some(std::time::Instant::now());
            return;
        }

        self.tts_audio_stream = Some(stream);
        self.speaking = true;
    }

    fn stop_recording_and_transcribe(&mut self) {
        // Drop the stream to stop recording
        self.audio_stream = None;
        self.recording = false;

        // Take the audio buffer
        let samples = {
            let mut buf = self.audio_buffer.lock().unwrap();
            std::mem::take(&mut *buf)
        };

        if samples.is_empty() {
            self.stt_error = Some("No audio captured".to_string());
            self.stt_error_time = Some(std::time::Instant::now());
            return;
        }

        // Load whisper model (lazy, first time only)
        let ctx = match self.load_whisper_model() {
            Ok(ctx) => ctx,
            Err(e) => {
                self.stt_error = Some(e);
                self.stt_error_time = Some(std::time::Instant::now());
                return;
            }
        };

        let source_rate = self.audio_sample_rate;
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = transcribe_audio(ctx, samples, source_rate);
            let _ = tx.send(result);
        });

        self.transcription_rx = Some(rx);
        self.transcribing = true;
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Helix/Colibri purple theme
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = Some(egui::Color32::from_rgb(0xa4, 0xa0, 0xe8)); // lavender
        visuals.panel_fill = egui::Color32::from_rgb(0x3b, 0x22, 0x4c);               // midnight purple
        visuals.window_fill = egui::Color32::from_rgb(0x3b, 0x22, 0x4c);
        visuals.extreme_bg_color = egui::Color32::from_rgb(0x28, 0x17, 0x33);         // revolver (TextEdit bg)
        visuals.faint_bg_color = egui::Color32::from_rgb(0x28, 0x17, 0x33);
        visuals.selection.bg_fill = egui::Color32::from_rgb(0x54, 0x00, 0x99);        // selection purple
        visuals.selection.stroke = egui::Stroke::new(0.0, egui::Color32::from_rgb(0xa4, 0xa0, 0xe8));
        visuals.warn_fg_color = egui::Color32::from_rgb(0xff, 0xcd, 0x1c);
        visuals.error_fg_color = egui::Color32::from_rgb(0xf4, 0x78, 0x68);
        for widgets in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widgets.bg_fill = egui::Color32::from_rgb(0x3b, 0x22, 0x4c);
            widgets.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0xa4, 0xa0, 0xe8));
        }
        // Brighter fg for active widgets so strong_text_color() is distinct from normal text
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, egui::Color32::from_rgb(0xe0, 0xdd, 0xff));
        ctx.set_visuals(visuals);

        // Update dirty flag once per frame
        self.dirty = self.content != self.saved_content;

        // Handle keyboard shortcuts
        let cmd = egui::Modifiers::MAC_CMD;

        if ctx.input_mut(|i| i.consume_key(cmd, egui::Key::S)) {
            self.save();
        }

        if ctx.input_mut(|i| i.consume_key(cmd, egui::Key::E)) {
            if self.dirty {
                self.show_exit_dialog = true;
            } else {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        if ctx.input_mut(|i| i.consume_key(cmd, egui::Key::P)) {
            self.preview_mode = !self.preview_mode;
        }

        if ctx.input_mut(|i| i.consume_key(cmd, egui::Key::R)) {
            if self.dirty {
                self.show_reload_dialog = true;
            } else {
                self.reload_file();
            }
        }

        // Cmd+D: toggle speech-to-text recording
        if ctx.input_mut(|i| i.consume_key(cmd, egui::Key::D)) {
            if self.transcribing {
                // Ignore while transcribing
            } else if self.recording {
                self.stop_recording_and_transcribe();
            } else {
                if let Err(e) = self.start_recording() {
                    self.stt_error = Some(e);
                    self.stt_error_time = Some(std::time::Instant::now());
                }
            }
        }

        // Cmd+T: toggle text-to-speech
        if ctx.input_mut(|i| i.consume_key(cmd, egui::Key::T)) {
            if self.speaking || self.generating_speech {
                // Stop playback / cancel generation
                self.stop_speaking();
                self.generating_speech = false;
                self.speech_rx = None;
            } else {
                // Get selected text, or fall back to full document
                let editor_id = egui::Id::new("editor");
                let text = if let Some(state) = egui::TextEdit::load_state(ctx, editor_id) {
                    if let Some(ccursor_range) = state.cursor.char_range() {
                        let start = ccursor_range.sorted_cursors()[0].index;
                        let end = ccursor_range.sorted_cursors()[1].index;
                        if start != end {
                            // Has selection
                            let chars: Vec<char> = self.content.chars().collect();
                            let s = start.min(chars.len());
                            let e = end.min(chars.len());
                            chars[s..e].iter().collect::<String>()
                        } else {
                            self.content.clone()
                        }
                    } else {
                        self.content.clone()
                    }
                } else {
                    self.content.clone()
                };

                let text = strip_markdown(text.trim());
                if !text.is_empty() {
                    self.start_speaking(text);
                }
            }
        }

        // Intercept window close if dirty (but not if user already confirmed)
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.dirty && !self.force_exit {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.show_exit_dialog = true;
            }
        }

        let was_preview = self.preview_mode != self.prev_preview_mode && !self.preview_mode;

        // Per-frame transcription polling
        if self.transcribing {
            ctx.request_repaint();
            if let Some(ref rx) = self.transcription_rx {
                match rx.try_recv() {
                    Ok(Ok(text)) => {
                        self.transcribing = false;
                        self.transcription_rx = None;
                        if text.is_empty() {
                            self.stt_error = Some("No speech detected".to_string());
                            self.stt_error_time = Some(std::time::Instant::now());
                        } else {
                            // Insert text at cursor position
                            let editor_id = egui::Id::new("editor");
                            let byte_idx = if let Some(state) = egui::TextEdit::load_state(ctx, editor_id) {
                                if let Some(ccursor_range) = state.cursor.char_range() {
                                    let idx = ccursor_range.primary.index;
                                    self.content.char_indices()
                                        .nth(idx)
                                        .map_or(self.content.len(), |(i, _)| i)
                                } else {
                                    self.content.len()
                                }
                            } else {
                                self.content.len()
                            };

                            // Add a leading space if we're not at start and previous char isn't whitespace
                            let needs_space = byte_idx > 0
                                && !self.content[..byte_idx].ends_with(char::is_whitespace);
                            let insert = if needs_space {
                                format!(" {text}")
                            } else {
                                text
                            };
                            self.content.insert_str(byte_idx, &insert);
                        }
                    }
                    Ok(Err(e)) => {
                        self.transcribing = false;
                        self.transcription_rx = None;
                        self.stt_error = Some(e);
                        self.stt_error_time = Some(std::time::Instant::now());
                    }
                    Err(mpsc::TryRecvError::Empty) => {}
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.transcribing = false;
                        self.transcription_rx = None;
                        self.stt_error = Some("Transcription thread crashed".to_string());
                        self.stt_error_time = Some(std::time::Instant::now());
                    }
                }
            }
        }

        // Per-frame TTS polling — drain all ready chunks
        if self.generating_speech {
            ctx.request_repaint();
            // Drain channel into a local vec to avoid borrow conflicts
            let mut chunks: Vec<Vec<f32>> = Vec::new();
            let mut tts_error: Option<String> = None;
            let mut disconnected = false;
            if let Some(ref rx) = self.speech_rx {
                loop {
                    match rx.try_recv() {
                        Ok(Ok(samples)) => {
                            chunks.push(tts::resample(&samples, 24000, self.tts_device_rate));
                        }
                        Ok(Err(e)) => { tts_error = Some(e); break; }
                        Err(mpsc::TryRecvError::Empty) => break,
                        Err(mpsc::TryRecvError::Disconnected) => { disconnected = true; break; }
                    }
                }
            }
            // Process collected chunks
            for resampled in chunks {
                if self.speaking {
                    if let Some(ref pb) = self.tts_playback {
                        pb.lock().unwrap().samples.extend_from_slice(&resampled);
                    }
                } else {
                    let pb = Arc::new(Mutex::new(TtsPlayback {
                        samples: resampled,
                        cursor: 0,
                        done: false,
                    }));
                    self.tts_playback = Some(Arc::clone(&pb));
                    self.start_audio_stream(pb);
                }
            }
            if let Some(e) = tts_error {
                self.generating_speech = false;
                self.speech_rx = None;
                self.stt_error = Some(e);
                self.stt_error_time = Some(std::time::Instant::now());
            } else if disconnected {
                self.generating_speech = false;
                self.speech_rx = None;
                if let Some(ref pb) = self.tts_playback {
                    pb.lock().unwrap().done = true;
                }
            }
        }

        // Check if TTS playback finished
        if self.speaking {
            ctx.request_repaint();
            if let Some(ref playback) = self.tts_playback {
                let state = playback.lock().unwrap();
                if state.cursor >= state.samples.len() && state.done {
                    drop(state);
                    self.stop_speaking();
                }
            }
        }

        // Update title when state changes
        if self.dirty != self.prev_dirty
            || self.preview_mode != self.prev_preview_mode
            || self.recording != self.prev_recording
            || self.transcribing != self.prev_transcribing
            || self.speaking != self.prev_speaking
            || self.generating_speech != self.prev_generating_speech
        {
            let name = self
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| self.path.display().to_string());
            let display_name = if name.len() > 40 {
                format!("{}...", &name[..37])
            } else {
                name
            };
            let mut title = format!("db - {}", display_name);
            if self.dirty {
                title.push_str(" [modified]");
            }
            if self.preview_mode {
                title.push_str(" [preview]");
            }
            if self.recording {
                title.push_str(" [recording]");
            }
            if self.transcribing {
                title.push_str(" [transcribing]");
            }
            if self.generating_speech {
                title.push_str(" [generating...]");
            }
            if self.speaking {
                title.push_str(" [speaking]");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
            self.prev_dirty = self.dirty;
            self.prev_preview_mode = self.preview_mode;
            self.prev_recording = self.recording;
            self.prev_transcribing = self.transcribing;
            self.prev_speaking = self.speaking;
            self.prev_generating_speech = self.generating_speech;
        }

        // Exit confirmation dialog
        if self.show_exit_dialog {
            // Check for dialog hotkeys, then drain all remaining keys/text
            // so the editor behind can't receive them.
            let (pressed_s, pressed_d, pressed_esc) = ctx.input_mut(|i| {
                let s = i.consume_key(egui::Modifiers::NONE, egui::Key::S);
                let d = i.consume_key(egui::Modifiers::NONE, egui::Key::D);
                let esc = i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                i.events.clear();
                (s, d, esc)
            });

            if pressed_s {
                self.save();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if pressed_d {
                self.force_exit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            } else if pressed_esc {
                self.show_exit_dialog = false;
            }

            egui::Window::new("Unsaved Changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(egui::Frame::window(ctx.style().as_ref()).fill(egui::Color32::BLACK))
                .show(ctx, |ui| {
                    ui.label("You have unsaved changes. What would you like to do?");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("[S]ave & Exit").clicked() {
                            self.save();
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("[D]iscard & Exit").clicked() {
                            self.force_exit = true;
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        if ui.button("Cancel (Esc)").clicked() {
                            self.show_exit_dialog = false;
                        }
                    });
                });
        }

        // Reload confirmation dialog
        if self.show_reload_dialog {
            let (pressed_r, pressed_esc) = ctx.input_mut(|i| {
                let r = i.consume_key(egui::Modifiers::NONE, egui::Key::R);
                let esc = i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                i.events.clear();
                (r, esc)
            });

            if pressed_r {
                self.show_reload_dialog = false;
                self.reload_file();
            } else if pressed_esc {
                self.show_reload_dialog = false;
            }

            egui::Window::new("Reload File")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(egui::Frame::window(ctx.style().as_ref()).fill(egui::Color32::BLACK))
                .show(ctx, |ui| {
                    ui.label("You have unsaved changes. Reload and discard them?");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.button("[R]eload").clicked() {
                            self.show_reload_dialog = false;
                            self.reload_file();
                        }
                        if ui.button("Cancel (Esc)").clicked() {
                            self.show_reload_dialog = false;
                        }
                    });
                });
        }

        // STT error popup
        if let Some(ref err) = self.stt_error.clone() {
            let should_dismiss = self
                .stt_error_time
                .is_some_and(|t| t.elapsed().as_secs() >= 5);
            if should_dismiss {
                self.stt_error = None;
                self.stt_error_time = None;
            } else {
                ctx.request_repaint();
                egui::Area::new(egui::Id::new("stt_error"))
                    .anchor(egui::Align2::CENTER_TOP, [0.0, 8.0])
                    .show(ctx, |ui| {
                        egui::Frame::popup(ui.style())
                            .fill(egui::Color32::from_rgb(0x44, 0x11, 0x11))
                            .show(ui, |ui| {
                                ui.colored_label(
                                    egui::Color32::from_rgb(0xf4, 0x78, 0x68),
                                    err,
                                );
                            });
                    });
            }
        }

        // Main editor panel with status bar at bottom
        let editor_id = egui::Id::new("editor");
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.show_exit_dialog || self.show_reload_dialog {
                ui.disable();
            }

            let editor_rect = ui.available_rect_before_wrap();
            let scroll_height = editor_rect.height();

            if self.preview_mode {
                egui::ScrollArea::vertical()
                    .max_height(scroll_height)
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                CommonMarkViewer::new().show(ui, &mut self.commonmark_cache, &self.content);
                            });
                    });
            } else {
                egui::ScrollArea::vertical()
                    .max_height(scroll_height)
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        let logical_line_count = {
                            let n = self.content.lines().count().max(1);
                            if self.content.ends_with('\n') { n + 1 } else { n }
                        };
                        let digit_count = format!("{}", logical_line_count).len();
                        let font_id = egui::TextStyle::Monospace.resolve(ui.style());
                        let muted = egui::Color32::from_rgb(0x5a, 0x59, 0x77);

                        let sample = "8".repeat(digit_count);
                        let gutter_text_width = ui.fonts_mut(|f| {
                            f.layout_no_wrap(sample, font_id.clone(), muted).size().x
                        });
                        let gutter_padding = 8.0;
                        let gutter_width = gutter_text_width + gutter_padding;

                        ui.horizontal_top(|ui| {
                            let (gutter_rect, _) = ui.allocate_exact_size(
                                egui::vec2(gutter_width, 0.0),
                                egui::Sense::hover(),
                            );
                            ui.add_space(4.0);

                            let available = ui.available_width();
                            let output = egui::TextEdit::multiline(&mut self.content)
                                .id(editor_id)
                                .font(egui::TextStyle::Monospace)
                                .desired_width(available)
                                .frame(false)
                                .show(ui);

                            let galley = &output.galley;
                            let galley_pos = output.galley_pos;
                            let painter = ui.painter();
                            let gutter_right_x = gutter_rect.right() - gutter_padding / 2.0;

                            let mut logical_line: usize = 1;
                            let mut prev_ended_with_newline = true;

                            for placed_row in &galley.rows {
                                if prev_ended_with_newline {
                                    let num_str = format!("{:>width$}", logical_line, width = digit_count);
                                    painter.text(
                                        egui::pos2(gutter_right_x, galley_pos.y + placed_row.pos.y),
                                        egui::Align2::RIGHT_TOP,
                                        num_str,
                                        font_id.clone(),
                                        muted,
                                    );
                                }
                                if placed_row.ends_with_newline {
                                    logical_line += 1;
                                    prev_ended_with_newline = true;
                                } else {
                                    prev_ended_with_newline = false;
                                }
                            }

                            if galley.rows.is_empty() {
                                painter.text(
                                    egui::pos2(gutter_right_x, galley_pos.y),
                                    egui::Align2::RIGHT_TOP,
                                    "1",
                                    font_id.clone(),
                                    muted,
                                );
                            }
                        });
                    });
            }
        });

        // Extract cursor position from TextEdit state
        if let Some(state) = egui::TextEdit::load_state(ctx, editor_id) {
            if let Some(ccursor_range) = state.cursor.char_range() {
                let idx = ccursor_range.primary.index;
                let byte_idx = self.content.char_indices()
                    .nth(idx)
                    .map_or(self.content.len(), |(i, _)| i);
                let before = &self.content[..byte_idx];
                self.cursor_line = before.matches('\n').count();
                self.cursor_col = before.len() - before.rfind('\n').map_or(0, |p| p + 1);
            }
        }

        if self.first_frame || was_preview {
            ctx.memory_mut(|m| m.request_focus(editor_id));
            self.first_frame = false;
        }
    }
}
