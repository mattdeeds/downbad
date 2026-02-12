# downbad

A minimal markdown editor for macOS built with Rust and egui.

```
db <filename>
```

Opens the file in a raw text editor with line numbers. The process detaches from the terminal so your shell prompt returns immediately.

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| Cmd+S | Save |
| Cmd+E | Exit (prompts if unsaved) |
| Cmd+P | Toggle markdown preview |
| Cmd+R | Reload file from disk |
| Cmd+D | Toggle speech-to-text recording |
| Cmd+C/V/X | Copy / Paste / Cut |

### Unsaved Changes Dialog

| Key | Action |
|-----|--------|
| S | Save & Exit |
| D | Discard & Exit |
| Esc | Cancel |

## Speech-to-Text

Cmd+D starts recording from the default microphone. Press Cmd+D again to stop and transcribe. The Whisper model is loaded lazily on first use, so it never slows down app startup.

### Setup (one-time)

```bash
mkdir -p ~/.local/share/downbad
curl -L -o ~/.local/share/downbad/ggml-base.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
```
