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
| Cmd+D | Toggle speech-to-text recording |
| Cmd+C/V/X | Copy / Paste / Cut |

### Unsaved Changes Dialog

| Key | Action |
|-----|--------|
| S | Save & Exit |
| D | Discard & Exit |
| Esc | Cancel |

## Speech-to-Text

Cmd+D starts recording from the default microphone. Press Cmd+D again to stop and transcribe. Requires a Whisper model at `~/.local/share/downbad/ggml-base.en.bin`.
