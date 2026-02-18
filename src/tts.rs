use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{mpsc, Mutex};

pub struct KokoroTts {
    session: Mutex<ort::session::Session>,
    vocab: HashMap<String, i64>,
    max_token_len: usize,
    voice_data: Vec<f32>,
}

impl KokoroTts {
    pub fn new() -> Result<Self, String> {
        let dir = data_dir();

        let model_path = dir.join("kokoro-v1.0.onnx");
        if !model_path.exists() {
            return Err(format!(
                "Kokoro model not found at {}. Download from HuggingFace onnx-community/Kokoro-82M-v1.0-ONNX.",
                model_path.display()
            ));
        }

        let session = ort::session::Session::builder()
            .map_err(|e| format!("ONNX session builder: {e}"))?
            .commit_from_file(&model_path)
            .map_err(|e| format!("ONNX model load: {e}"))?;

        let tokenizer_path = dir.join("kokoro-tokenizer.json");
        if !tokenizer_path.exists() {
            return Err(format!(
                "Tokenizer not found at {}",
                tokenizer_path.display()
            ));
        }
        let json_str =
            fs::read_to_string(&tokenizer_path).map_err(|e| format!("Read tokenizer: {e}"))?;
        let tokenizer_json: serde_json::Value =
            serde_json::from_str(&json_str).map_err(|e| format!("Parse tokenizer: {e}"))?;
        let vocab_obj = tokenizer_json["model"]["vocab"]
            .as_object()
            .ok_or("Invalid tokenizer: missing model.vocab")?;
        let vocab: HashMap<String, i64> = vocab_obj
            .iter()
            .map(|(k, v)| (k.clone(), v.as_i64().unwrap_or(0)))
            .collect();
        let max_token_len = vocab.keys().map(|k| k.chars().count()).max().unwrap_or(1);

        let voice_path = dir.join("kokoro-voice.bin");
        if !voice_path.exists() {
            return Err(format!(
                "Voice style not found at {}",
                voice_path.display()
            ));
        }
        let voice_bytes = fs::read(&voice_path).map_err(|e| format!("Read voice: {e}"))?;
        let voice_data: Vec<f32> = voice_bytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        Ok(Self {
            session: Mutex::new(session),
            vocab,
            max_token_len,
            voice_data,
        })
    }

    #[allow(dead_code)]
    pub fn synthesize(&self, text: &str) -> Result<Vec<f32>, String> {
        let sentences = split_sentences(text);
        let mut all_audio = Vec::new();

        for sentence in &sentences {
            if let Some(audio) = self.synthesize_sentence(sentence)? {
                all_audio.extend_from_slice(&audio);
            }
        }

        if all_audio.is_empty() {
            return Err("No audio produced from text".to_string());
        }
        Ok(all_audio)
    }

    pub fn synthesize_streaming(&self, text: &str, tx: mpsc::Sender<Result<Vec<f32>, String>>) {
        let sentences = split_sentences(text);
        for sentence in &sentences {
            match self.synthesize_sentence(sentence) {
                Ok(Some(audio)) => {
                    if tx.send(Ok(audio)).is_err() {
                        return;
                    }
                }
                Ok(None) => {}
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            }
        }
    }

    fn synthesize_sentence(&self, sentence: &str) -> Result<Option<Vec<f32>>, String> {
        let ipa = phonemize(sentence)?;
        let misaki = espeak_ipa_to_misaki(&ipa);
        let inner_tokens = self.tokenize(&misaki);
        if inner_tokens.is_empty() {
            return Ok(None);
        }

        let inner_tokens = if inner_tokens.len() > 510 {
            &inner_tokens[..510]
        } else {
            &inner_tokens[..]
        };

        let style = self.get_style_vector(inner_tokens.len());

        let mut tokens = Vec::with_capacity(inner_tokens.len() + 2);
        tokens.push(0i64);
        tokens.extend_from_slice(inner_tokens);
        tokens.push(0i64);

        let audio = self.run_inference(&tokens, &style)?;
        Ok(Some(audio))
    }

    fn run_inference(&self, tokens: &[i64], style: &[f32]) -> Result<Vec<f32>, String> {
        let n = tokens.len();

        let input_ids =
            ort::value::Tensor::from_array(([1usize, n], tokens.to_vec()))
                .map_err(|e| format!("Create input_ids tensor: {e}"))?;
        let style_tensor =
            ort::value::Tensor::from_array(([1usize, 256usize], style.to_vec()))
                .map_err(|e| format!("Create style tensor: {e}"))?;
        let speed =
            ort::value::Tensor::from_array(([1usize], vec![1.0f32]))
                .map_err(|e| format!("Create speed tensor: {e}"))?;

        let mut session = self.session.lock().map_err(|e| format!("Session lock: {e}"))?;
        let outputs = session
            .run(ort::inputs! {
                "input_ids" => input_ids,
                "style" => style_tensor,
                "speed" => speed,
            })
            .map_err(|e| format!("ONNX inference: {e}"))?;

        let audio = outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|e| format!("Extract audio: {e}"))?;

        Ok(audio.1.to_vec())
    }

    fn tokenize(&self, text: &str) -> Vec<i64> {
        let chars: Vec<char> = text.chars().collect();
        let mut ids = Vec::new();
        let mut i = 0;

        while i < chars.len() {
            let limit = self.max_token_len.min(chars.len() - i);
            let mut matched = false;

            for l in (1..=limit).rev() {
                let candidate: String = chars[i..i + l].iter().collect();
                if let Some(&id) = self.vocab.get(&candidate) {
                    ids.push(id);
                    i += l;
                    matched = true;
                    break;
                }
            }

            if !matched {
                // Skip unknown characters
                i += 1;
            }
        }

        ids
    }

    fn get_style_vector(&self, token_count: usize) -> Vec<f32> {
        let offset = token_count * 256;
        if offset + 256 <= self.voice_data.len() {
            self.voice_data[offset..offset + 256].to_vec()
        } else if self.voice_data.len() >= 256 {
            self.voice_data[0..256].to_vec()
        } else {
            vec![0.0; 256]
        }
    }
}

fn data_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".local/share/downbad")
}

fn ensure_espeak_data() {
    if std::env::var("PIPER_ESPEAKNG_DATA_DIRECTORY").is_ok() {
        return;
    }
    // Homebrew on Apple Silicon
    let candidates = [
        "/opt/homebrew/Cellar/espeak-ng/1.52.0/share",
        "/opt/homebrew/share",
        "/usr/local/share",
        "/usr/share",
    ];
    for dir in &candidates {
        let path = PathBuf::from(dir).join("espeak-ng-data");
        if path.exists() {
            // Safety: called early, before any other threads use this env var.
            unsafe { std::env::set_var("PIPER_ESPEAKNG_DATA_DIRECTORY", dir); }
            return;
        }
    }
}

fn phonemize(text: &str) -> Result<String, String> {
    ensure_espeak_data();
    let phonemes = espeak_rs::text_to_phonemes(text, "en-us", None, true, false)
        .map_err(|e| format!("Phonemization error: {e:?}"))?;
    Ok(phonemes.join(" "))
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for c in text.chars() {
        current.push(c);
        if matches!(c, '.' | '!' | '?' | '\n') {
            let trimmed = current.trim().to_string();
            if !trimmed.is_empty() {
                sentences.push(trimmed);
            }
            current.clear();
        }
    }

    let trimmed = current.trim().to_string();
    if !trimmed.is_empty() {
        sentences.push(trimmed);
    }

    if sentences.is_empty() {
        sentences.push(text.to_string());
    }

    sentences
}

fn espeak_ipa_to_misaki(ipa: &str) -> String {
    // Step 1: Replace Unicode tie bar U+0361 with caret for easier matching
    let mut result = ipa.replace('\u{0361}', "^");

    // Step 2: Apply replacements (longest first to avoid partial matches)
    // Include both with-tie-bar (^) and without variants for robustness
    let replacements = [
        // Glottal stop + syllabic n
        ("ʔˌn\u{0329}", "tᵊn"),
        // Diphthongs (with tie bar)
        ("a^ɪ", "I"),
        ("a^ʊ", "W"),
        ("e^ɪ", "A"),
        ("ɔ^ɪ", "Y"),
        ("o^ʊ", "O"),
        // Diphthongs (without tie bar, in case espeak omits them)
        ("aɪ", "I"),
        ("aʊ", "W"),
        ("eɪ", "A"),
        ("ɔɪ", "Y"),
        ("oʊ", "O"),
        // Affricates (with tie bar)
        ("d^ʒ", "ʤ"),
        ("t^ʃ", "ʧ"),
        // Affricates (without tie bar)
        ("dʒ", "ʤ"),
        ("tʃ", "ʧ"),
        // Syllabic l (with tie bar only — without tie bar would over-match)
        ("ə^l", "ᵊl"),
        // Glottal stop + n
        ("ʔn", "tᵊn"),
        // Rhotacized schwa
        ("ɚ", "əɹ"),
        // Palatalization before diphthongs
        ("ʲO", "jO"),
        ("ʲQ", "jQ"),
        // Combining tilde — remove
        ("\u{0303}", ""),
        // American English vowel mappings
        ("ɜːɹ", "ɜɹ"),
        ("ɜː", "ɜɹ"),
        ("ɪə", "iə"),
        // Single-char replacements (must come after multi-char ones)
        ("e", "A"),
        ("r", "ɹ"),
        ("x", "k"),
        ("ç", "k"),
        ("ɐ", "ə"),
        ("ɬ", "l"),
        ("ʔ", "t"),
        ("ʲ", ""),
    ];

    for (old, new_val) in &replacements {
        result = result.replace(old, new_val);
    }

    // Step 3: Handle syllabic consonants (combining mark U+0329)
    // Pattern: consonant + U+0329 → ᵊ + consonant
    let mut chars: Vec<char> = result.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 1 < chars.len() && chars[i + 1] == '\u{0329}' {
            let consonant = chars[i];
            chars[i] = 'ᵊ';
            chars[i + 1] = consonant;
            i += 2;
        } else {
            i += 1;
        }
    }
    result = chars.into_iter().collect();
    result = result.replace('\u{0329}', "");

    // Step 4: Remove all remaining length marks
    result = result.replace('ː', "");

    // Step 5: Remove remaining tie/caret markers
    result = result.replace('^', "");

    result
}

/// Resample audio from source_rate to target_rate using linear interpolation.
pub fn resample(samples: &[f32], source_rate: u32, target_rate: u32) -> Vec<f32> {
    if source_rate == target_rate {
        return samples.to_vec();
    }
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
}
