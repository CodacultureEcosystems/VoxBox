# VoxBox — a Voice Station for the Linux PC (Rust)

A speech-to-text (STT) and speech-to-speech (STS) Station that works as a shell
for your entire Linux PC — and a system-wide dictation Being for every
application on the desktop, with a real settings window (**VoxBox Studio**) and
a desktop tile you can click, double-click, and hold to talk. Fully local by
default. **All Rust, zero Python.**

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

- **STT — two engines:** whisper.cpp via `whisper-rs` (99 languages) **or**
  NVIDIA **Parakeet** (`parakeet-tdt-0.6b-v3` via the pure-Rust `burn` backend
  — no ONNX Runtime; fast, auto-punctuation, 25 languages auto-detected).
  Switch between them in the Studio window; models auto-download on first use,
  and **local model paths are accepted** (point `[stt] model` at any `ggml-*.bin`
  for whisper, or any exported ONNX folder for parakeet).
- **Dictation (system-wide):** press the hotkey, speak, and the text lands in
  whatever application currently has focus — editor, browser, chat, anything.
  Injection: native `/dev/uinput` first, falling back to `wtype`, `xdotool`, or
  `ydotool`.
- **VoxBox Studio (window UI):** left-click the tray Being (or launch `vbd
  --window`) for a real window — status banner, one-click record, conversation
  launchers, Piper voice picker with a live test line, **speech-engine
  selector, local-model loader, AI-agent picker, and the desktop tile**.
- **Desktop tile:** a small always-on-top VoxBox icon. **Single click** records
  one utterance, **double click** toggles constant mode (continuous dictated
  conversation), **hold and release** is push-to-talk.
- **Tray Being:** while the dictation resident runs, a status icon (SNI over
  D-Bus) shows idle/recording state and offers *Record now*, a constant-mode
  toggle, *Open VoxBox Studio*, *Quit*.
- **Dialogue / shell:** every utterance becomes either a shell command executed
  on your PC or a conversational spoken answer (rule parser built in; plug in
  any OpenAI-compatible LLM for full speech-to-speech conversation).
- **AI agents:** drop one `.toml` file per agent into `~/.config/voxbox/agents/`
  (`name`, `api_base`, `api_key`, `model`) and pick the active one in Studio
  — the `[brain]` routes LLM requests there.
- **TTS:** piper (bundled by the installer) / spd-say / espeak-ng,
  auto-detected.
- **Safety gate:** blocklist + confirmation for destructive commands.
- **Witnesses:** every transition is recorded as a 32-byte TraceEvent
  (TRES_ABI-frozen layout) plus a non-empty witness line in `trace.trace`.

## Install (bundled Whisper + Piper)

```bash
# fresh install — the whole thing in five lines
git clone https://github.com/CodacultureEcosystems/VoxBox.git
cd VoxBox
./install.sh
sudo usermod -aG input,uinput "$USER"   # then log out and back in

# optional: pre-fetch the Parakeet model now instead of on first use
./install.sh --with-parakeet   # ~2.5 GB, pure-Rust burn backend, no ORT
```

### Upgrading (or replacing an old copy)

Everything VoxBox installs lives under your home directory, so removal is
one block — no system packages to uninstall:

```bash
pkill -f vbd 2>/dev/null; pkill -f voxbox 2>/dev/null
rm -rf ~/.local/bin/voxbox ~/.local/bin/vbd ~/.local/bin/piper \
       ~/.config/voxbox ~/.local/share/voxbox ~/.cache/voxbox \
       ~/.local/share/applications/voxbox.desktop \
       ~/.config/autostart/voxbox.desktop \
       ~/.local/share/icons/hicolor/scalable/apps/voxbox.svg \
       ~/.local/share/icons/hicolor/512x512/apps/voxbox.png

# then re-clone at the latest tag and reinstall
git clone --branch v0.2.1 --depth 1 \
    https://github.com/CodacultureEcosystems/VoxBox.git VoxBox
cd VoxBox && ./install.sh
sudo usermod -aG input,uinput "$USER"   # then log out and back in
```

### What the installer does

Bash only — no Python anywhere in the project or the installer. The script:

1. installs system packages (alsa, cmake, toolchain — apt/dnf/pacman aware);
2. builds the Rust binaries with `cargo` (whisper.cpp sources compile in-tree);
3. downloads a Whisper ggml model to `~/.cache/voxbox/` (default `base`,
   override with `VOXBOX_MODEL=tiny|base|small`);
4. downloads the Piper release tarball + a voice (`en_US-lessac-medium`) and
   wires them into the config;
5. installs the menu entry (`VoxBox` in your desktop menu, with the VoxBox
   icon; right-click the panel button to pick: dictation resident / voice
   shell / text mode) and icon files under `~/.local/share/icons/hicolor/`.

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
| `src/stt.rs`         | whisper + parakeet transcription, model paths + download |
| `src/dictate.rs`     | the vbd Being: signals → record → transcribe → inject |
| `src/keys.rs`        | ASCII → evdev keycode map (US-QWERTY)            |
| `src/inject.rs`      | uinput virtual keyboard + wtype/xdotool/ydotool  |
| `src/trace.rs`       | TRES_ABI TraceEvent witness log                  |
| `src/brain.rs`       | rule parser + optional LLM intent decision       |
| `src/executor.rs`    | safety-gated command execution                   |
| `src/tts.rs`         | piper / spd-say / espeak spoken replies          |
| `src/shell.rs`       | interactive voice shell loop                     |
| `src/config.rs`      | TOML config with sane defaults                   |
| `src/studio.rs`      | VoxBox Studio window + desktop tile (egui)       |
| `src/tray.rs`        | SNI tray Being: state icon, menu, window launcher |

## Development

```bash
cargo test          # 71 unit tests (brain, executor, vad, config, tts, keys, inject, tile)
cargo build --release
```

## License

Dual-licensed under **MIT OR Apache-2.0** (`LICENSE-MIT`, `LICENSE-APACHE`) —
you may choose either. The Apache grants an explicit patent license; the MIT
keeps redistribution simple.

## Lion Rule — honesty about what is verified

- **Verified in this repository / sandbox:** the crate compiles clean (no
  warnings), all 71 unit tests pass; hotkey-code map, ASCII→keycode map,
  injector argument shapes, tray state/label/pixmap helpers, tile pointer
  gestures (pure state machine), config set-subkey/agent loading, and the
  frozen 32-byte TraceEvent layout (size/align asserted) are unit-tested.
- **Parakeet verified end-to-end in the sandbox:** the pure-Rust `burn`
  backend loaded `parakeet-tdt-0.6b-v3` and correctly transcribed a real 11 s
  speech sample ("And so, my fellow Americans, ask not what your country can
  do for you…") with no ONNX Runtime — the engine works on a stock Debian
  box without `pkg-config`/OpenSSL.
- **Not verified here (needs a real desktop session with audio + input
  devices):** end-to-end global hotkey capture from `/dev/input/event*`,
  uinput injection into a focused window, desktop notifications, tray menu and
  Studio/tile window interactions in a live desktop environment, and live
  dictation. This sandbox has no audio, input, or D-Bus session. Test on your
  machine and report what breaks — that is the Lion rule we hold.
