# VoxBox — a Voice Station for the Linux PC (Rust)

A speech-to-text (STT) and speech-to-speech (STS) Station that works as a shell
for your entire Linux PC — and a system-wide dictation Being for every
application on the desktop. Fully local by default. **All Rust, zero Python.**

## The Beings

VoxBox is a Station in the HIVE. Its residents follow the naming rule
(three-letter-d domain shorthand, `B + Kin + Family + Role @ Domain`):

| Signature  | Kin    | Family      | Role   | Home           | What it does |
| ---------- | ------ | ----------- | ------ | -------------- | ------------ |
| `Bsew@vbd` | Seer   | Witnessing  | Worker | VoxBox Station | Watches the global hotkey, records your voice, transcribes it locally, and types the text into whatever app is focused (WisprFlow-style dictation). |
| `Bvox@vbd` | Weaver | Translating | Worker | VoxBox Station | The interactive voice shell: listens, thinks, executes commands on the whole PC, and speaks replies back. Runs as the `voxbox` binary. |

The `vbd` binary (`Bsew@vbd`) is a **background resident, not a daemon** — it
sits at the VoxBox Station, listens for its hotkey (default: Scroll Lock), and
emits a non-empty trace witness for every transition it makes.

## What it does

```
mic ─ arecord ──► Rust VAD ──► whisper-rs ──► brain ──► executor ──► stdout + TTS
                                              ▲           └── spoken reply ──┘

hotkey ──► vbd (Bsew@vbd): record ──► whisper-rs ──► uinput/wtype inject
                └── every transition writes a witness to ~/.local/share/voxbox/trace.trace
```

- **STT:** whisper.cpp via `whisper-rs` — compiled from source, runs on CPU,
  model auto-downloads from HuggingFace on first run.
- **Dictation (system-wide):** press the hotkey, speak, and the text lands in
  whatever application currently has focus — editor, browser, chat, anything.
  Injection: native `/dev/uinput` first, falling back to `wtype`, `xdotool`, or
  `ydotool`.
- **Dialogue / shell:** every utterance becomes either a shell command executed
  on your PC or a conversational spoken answer (rule parser built in; plug in
  any OpenAI-compatible LLM for full speech-to-speech conversation).
- **TTS:** piper (bundled by the installer) / spd-say / espeak-ng,
  auto-detected.
- **Safety gate:** blocklist + confirmation for destructive commands.
- **Witnesses:** every transition is recorded as a 32-byte TraceEvent
  (TRES_ABI-frozen layout) plus a non-empty witness line in `trace.trace`.

## Install (bundled Whisper + Piper)

```bash
./install.sh
```

Bash only — no Python anywhere in the project or the installer. The script:

1. installs system packages (alsa, cmake, toolchain — apt/dnf/pacman aware);
2. builds the Rust binaries with `cargo` (whisper.cpp sources compile in-tree);
3. downloads a Whisper ggml model to `~/.cache/voxbox/` (default `base`,
   override with `VOXBOX_MODEL=tiny|base|small`);
4. downloads the Piper release tarball + a voice (`en_US-lessac-medium`) and
   wires them into the config;
5. installs the menu entry (`VoxBox` in your desktop menu, with the VoxBox
   icon) and icon files under `~/.local/share/icons/hicolor/`.

**Permissions:** `vbd` needs to read global key events and synthesize typing:

```bash
sudo usermod -aG input,uinput "$USER"   # then log out and back in
```

## Manual quick start

```bash
sudo apt install alsa-utils espeak-ng
cargo build --release
./target/release/voxbox           # voice mode: press Enter, talk, pause
./target/release/voxbox --text    # typing only (no mic needed)
./target/release/vbd --help       # the dictation resident
./target/release/voxbox dictation # same Being via the main binary
```

Choose a different Whisper size with `--model tiny|base|small|medium|large-v3`
(`tiny` = fastest, `large-v3` = most accurate).

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
| `src/dictate.rs`     | the vbd Being: hotkey → record → transcribe → inject |
| `src/keys.rs`        | ASCII → evdev keycode map (US-QWERTY)            |
| `src/inject.rs`      | uinput virtual keyboard + wtype/xdotool/ydotool  |
| `src/trace.rs`       | TRES_ABI TraceEvent witness log                  |
| `src/brain.rs`       | rule parser + optional LLM intent decision       |
| `src/executor.rs`    | safety-gated command execution                   |
| `src/tts.rs`         | piper / spd-say / espeak spoken replies          |
| `src/shell.rs`       | interactive voice shell loop                     |
| `src/config.rs`      | TOML config with sane defaults                   |

## Development

```bash
cargo test          # 48 unit tests (brain, executor, vad, config, tts, keys, inject, trace)
cargo build --release
```

## License

Dual-licensed under **MIT OR Apache-2.0** (`LICENSE-MIT`, `LICENSE-APACHE`) —
you may choose either. The Apache grants an explicit patent license; the MIT
keeps redistribution simple.

## Lion Rule — honesty about what is verified

- **Verified in this repository:** the crate compiles clean (no warnings), all
  48 unit tests pass, and the hotkey-code map, ASCII→keycode map, injector
  argument shapes, and the frozen 32-byte TraceEvent layout (size/align
  asserted) are unit-tested.
- **Not verified here (needs a real desktop session with audio + input
  devices):** end-to-end global hotkey capture from `/dev/input/event*`,
  uinput injection into a focused window, desktop notifications, and live
  dictation. This sandbox has no audio or input devices. Test on your machine
  and report what breaks — that is the Lion rule we hold.
