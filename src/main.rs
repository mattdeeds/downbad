use eframe::egui;
use std::fs;
use std::path::PathBuf;

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

    // Fork to detach from the terminal so the shell prompt returns immediately.
    unsafe {
        let pid = libc::fork();
        if pid > 0 {
            // Parent: exit so the shell gets its prompt back.
            std::process::exit(0);
        } else if pid == 0 {
            // Child: start a new session to fully detach from the terminal.
            libc::setsid();
            // Redirect stdin/stdout/stderr to /dev/null so macOS framework
            // log messages don't spew into the terminal.
            let devnull = libc::open(b"/dev/null\0".as_ptr() as *const _, libc::O_RDWR);
            if devnull >= 0 {
                libc::dup2(devnull, 0);
                libc::dup2(devnull, 1);
                libc::dup2(devnull, 2);
                libc::close(devnull);
            }
        }
        // pid == -1: fork failed; just continue in the current process.
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([800.0, 600.0]),
        ..Default::default()
    };

    eframe::run_native(
        &format!("db - {}", path.display()),
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(path, content)))),
    )
}

struct App {
    path: PathBuf,
    content: String,
    saved_content: String,
    dirty: bool,
    prev_dirty: bool,
    first_frame: bool,
    show_exit_dialog: bool,
    force_exit: bool,
    cursor_line: usize,
    cursor_col: usize,
}

impl App {
    fn new(path: PathBuf, content: String) -> Self {
        Self {
            path,
            saved_content: content.clone(),
            content,
            dirty: false,
            prev_dirty: false,
            first_frame: true,
            show_exit_dialog: false,
            force_exit: false,
            cursor_line: 0,
            cursor_col: 0,
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
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Helix/Colibri purple theme
        let mut visuals = egui::Visuals::dark();
        visuals.override_text_color = None;
        visuals.panel_fill = egui::Color32::from_rgb(0x3b, 0x22, 0x4c);               // midnight purple
        visuals.window_fill = egui::Color32::from_rgb(0x3b, 0x22, 0x4c);
        visuals.extreme_bg_color = egui::Color32::from_rgb(0x28, 0x17, 0x33);         // revolver (TextEdit bg)
        visuals.faint_bg_color = egui::Color32::from_rgb(0x28, 0x17, 0x33);
        visuals.selection.bg_fill = egui::Color32::from_rgb(0x54, 0x00, 0x99);        // selection purple
        visuals.selection.stroke = egui::Stroke::new(0.0, egui::Color32::WHITE);
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

        // Intercept window close if dirty (but not if user already confirmed)
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.dirty && !self.force_exit {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.show_exit_dialog = true;
            }
        }

        // Update title only when dirty state changes
        if self.dirty != self.prev_dirty {
            let title = if self.dirty {
                format!("db - {} [modified]", self.path.display())
            } else {
                format!("db - {}", self.path.display())
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
            self.prev_dirty = self.dirty;
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

        // Main editor panel with status bar at bottom
        let editor_id = egui::Id::new("editor");
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.show_exit_dialog {
                ui.disable();
            }

            let editor_rect = ui.available_rect_before_wrap();
            let scroll_height = editor_rect.height();

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
                            .text_color(egui::Color32::from_rgb(0xa4, 0xa0, 0xe8)) // lavender
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

            // Status bar at bottom of central panel (disabled for now)
            // ui.horizontal(|ui| {
            //     let mono = egui::TextStyle::Monospace;
            //     let muted = egui::Color32::from_rgb(0x5a, 0x59, 0x77); // comet
            //     let display_path = {
            //         let fname = self.path.file_name().unwrap_or_default().to_string_lossy();
            //         match self.path.parent().and_then(|p| p.file_name()) {
            //             Some(dir) => format!("{}/{}", dir.to_string_lossy(), fname),
            //             None => fname.into_owned(),
            //         }
            //     };
            //     ui.label(egui::RichText::new(display_path).text_style(mono.clone()).color(muted));
            //     ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            //         let lines = self.content.lines().count().max(1);
            //         let ln = self.cursor_line + 1;
            //         let col = self.cursor_col + 1;
            //         ui.label(egui::RichText::new(
            //             format!("Ln {ln}, Col {col}  |  {lines} lines")
            //         ).text_style(mono).color(muted));
            //     });
            // });
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

        if self.first_frame {
            ctx.memory_mut(|m| m.request_focus(editor_id));
            self.first_frame = false;
        }
    }
}
