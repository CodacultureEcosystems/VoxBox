# VoxBox — a voice-first shell for Linux (Rust)

A speech-to-text (STT) and speech-to-speech (STS) application that works as a
shell for your entire Linux PC. One Rust binary, fully local by default:

- **STT:** whisper.cpp via `whisper-rs` — compiled from source, runs on CPU,
  models auto-download from HuggingFace on first run.
- **Dialogue:** every utterance becomes either a shell command executed on your
  PC or a conversational spoken answer (rule parser built in; plug in any
  OpenAI-compatible LLM for real speech-to-speech conversation).
- **TTS:** piper / spd-say / espeak-ng, auto-detected, replies spoken back.
- **Safety gate:** blocklist + confirmation for destructive commands.

```
mic ─ arecord ─► Rust VAD ─► whisper-rs ─► brain ─► executor ─► stdout + TTS
                                              ▲         └─ spoken reply ─┘
```

## Quick start

```bash
# system deps (Debian/Ubuntu): audio capture + at least one TTS backend
sudo apt install alsa-utils espeak-ng
# for the best offline voices, also install piper: https://github.com/rhasspy/piper

cargo build --release
./target/release/voxbox --help
./target/release/voxbox           # voice mode: press Enter, talk, pause
./target/release/voxbox --text    # typing only (no mic needed)
```

First voice-mode run downloads the Whisper `base` model (~148 MB) to
`~/.cache/voxbox/`. Choose a different size with `--model tiny|base|small|medium|large-v3`
or in the config (`tiny` = fastest, `large-v3` = most accurate).

### Building

Rust + cmake required (`apt install build-essential cmake`). If bindgen cannot
find libclang, either install it (`sudo apt install libclang-dev`) or build
with `WHISPER_DONT_GENERATE_BINDINGS=1`.

## Talking to it

| You say / type                            | It does                                    |
| ----------------------------------------- | ------------------------------------------ |
| "run df -h" / `df -h`                     | executes it, shows + speaks the output     |
| "open firefox"                            | launches Firefox in the background         |
| "what time is it"                         | runs `date`, speaks the time               |
| "hello" / "what is an inode" (with LLM)   | answers by voice, runs nothing             |
| "exit"                                    | quits the shell                            |

Unrecognized phrases are treated as shell commands — because it *is* a shell.
Risky commands (`rm`, `shutdown`, `sudo`, `kill`, …) ask `[y/N]` first;
fork bombs, `mkfs`, and `rm -rf /` are refused outright.

## Full spoken dialogue (LLM brain)

Edit `~/.config/voxbox/config.toml` (see `config.example.toml`):

```toml
[brain]
api_base = "http://127.0.0.1:11434/v1"   # local ollama, llama.cpp server, LM Studio…
model = "llama3.2"
# or a cloud endpoint (https://api.openai.com/v1); key via env:
# export VOXBOX_API_KEY=sk-...
```

With an LLM configured, the shell runs the transcribed text through the model,
which decides per utterance whether to run a command or to converse — giving a
complete speech-to-speech loop over your PC.

## Safety model

Destructive speech must survive two gates:

1. **Blocklist** — always refused (fork bombs, `mkfs`, `rm -rf /`, `sudo rm -rf ~`, …).
2. **Confirmation list** — `rm`, `shutdown`, `sudo`, `kill`, `git push`, … ask
   `run '…'? [y/N]` before executing.

Commands run as your own user with a timeout (default 20 s).

## Project layout

| File                 | Role                                             |
| -------------------- | ------------------------------------------------ |
| `src/capture.rs`     | arecord capture + end-of-utterance detection     |
| `src/vad.rs`         | pure-Rust energy voice-activity detector         |
| `src/stt.rs`         | whisper-rs transcription + model download        |
| `src/brain.rs`       | rule parser + optional LLM intent decision       |
| `src/executor.rs`    | safety-gated command execution                   |
| `src/tts.rs`         | piper / spd-say / espeak spoken replies          |
| `src/shell.rs`       | interactive voice shell loop                     |
| `src/config.rs`      | TOML config with sane defaults                   |

## Development

```bash
cargo test          # 33 unit tests (brain, executor, vad, config, tts)
cargo build --release
```
