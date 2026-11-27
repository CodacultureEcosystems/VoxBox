#!/usr/bin/env bash
# VoxBox Station installer. Bash only — no Python anywhere.
# Installs: system deps, Whisper (whisper-rs build + model), Piper TTS
# (binary + default voice), menu entry, icon, and optional autostart.
set -euo pipefail

REPO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODEL="${VOXBOX_MODEL:-base}"
PIPER_RELEASE="2023.11.14-2"
VOICE_REPO_PATH="v1.0.0/en/en_US/lessac/medium"
VOICE_NAME="en_US-lessac-medium"
INSTALL_BIN="${VOXBOX_BIN_DIR:-$HOME/.local/bin}"
DATA_DIR="$HOME/.local/share/voxbox"
CONFIG_DIR="$HOME/.config/voxbox"
CACHE_DIR="$HOME/.cache/voxbox"
WITH_PARAKEET=0
for arg in "$@"; do
    case "$arg" in
        --with-parakeet) WITH_PARAKEET=1 ;;
        *) echo "unknown option: $arg (supported: --with-parakeet)" >&2; exit 1 ;;
    esac
done

say() { printf '\n  \033[1;36m==>\033[0m %s\n' "$*"; }
fetch() {
    local url="$1" out="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fL --retry 3 -o "$out" "$url"
    elif command -v wget >/dev/null 2>&1; then
        wget -O "$out" "$url"
    else
        echo "need curl or wget to download models" >&2; exit 1
    fi
}

# ── 1. system dependencies ────────────────────────────────────────────────
if command -v apt-get >/dev/null 2>&1; then
    say "Installing system packages (alsa, cmake, toolchain)"
    sudo apt-get update -qq
    sudo apt-get install -y build-essential cmake libclang-dev alsa-utils \
        espeak-ng libasound2-dev xdotool
elif command -v dnf >/dev/null 2>&1; then
    say "Installing system packages (alsa, cmake, toolchain)"
    sudo dnf install -y gcc gcc-c++ cmake clang alsa-lib-devel alsa-utils \
        espeak-ng xdotool
elif command -v pacman >/dev/null 2>&1; then
    say "Installing system packages (alsa, cmake, toolchain)"
    sudo pacman -S --noconfirm base-devel cmake clang alsa-lib alsa-utils \
        espeak-ng xdotool
else
    echo "  (skip) unknown distro: install build tools + alsa-utils + espeak-ng manually" >&2
fi

if ! command -v cargo >/dev/null 2>&1; then
    say "Installing Rust (rustup)"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# ── 2. build (Rust only: whisper.cpp + the Parakeet engine compile in) ────
say "Building voxbox + vbd (release)"
(cd "$REPO_DIR" && cargo build --release)

# ── 2b. Parakeet model (optional now; also auto-downloads on first use) ───
PARAKEET_DIR="$CACHE_DIR/parakeet-tdt-0.6b-v3"
if [ "$WITH_PARAKEET" = "1" ]; then
    say "Downloading Parakeet (parakeet-tdt-0.6b-v3, ~2.5 GB)"
    mkdir -p "$PARAKEET_DIR"
    for f in encoder-model.onnx encoder-model.onnx.data decoder_joint-model.onnx vocab.txt; do
        [ -f "$PARAKEET_DIR/$f" ] || fetch \
            "https://huggingface.co/altunenes/parakeet-rs/resolve/main/parakeet-tdt-0.6b-v3/$f" \
            "$PARAKEET_DIR/$f"
    done
fi

# ── 3. install binaries ───────────────────────────────────────────────────
say "Installing binaries to $INSTALL_BIN"
mkdir -p "$INSTALL_BIN"
install -m 0755 "$REPO_DIR/target/release/voxbox" "$INSTALL_BIN/voxbox"
install -m 0755 "$REPO_DIR/target/release/vbd" "$INSTALL_BIN/vbd"
case ":$PATH:" in
    *":$INSTALL_BIN:"*) ;;
    *) echo "  note: add $INSTALL_BIN to your PATH" ;;
esac

# ── 4. Whisper model (pre-download so first run is instant) ───────────────
say "Downloading Whisper $MODEL model to ~/.cache/voxbox"
mkdir -p "$HOME/.cache/voxbox"
fetch "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-$MODEL.bin" \
      "$HOME/.cache/voxbox/ggml-$MODEL.bin.part" || echo "  (skip) model will download on first run"
[ -f "$HOME/.cache/voxbox/ggml-$MODEL.bin.part" ] && \
    mv "$HOME/.cache/voxbox/ggml-$MODEL.bin.part" "$HOME/.cache/voxbox/ggml-$MODEL.bin"

# ── 5. Piper TTS (official release binary + default voice) ────────────────
say "Installing Piper TTS"
ARCH="$(uname -m)"
case "$ARCH" in
    x86_64)  PIPER_TAR="piper_linux_x86_64.tar.gz" ;;
    aarch64) PIPER_TAR="piper_linux_aarch64.tar.gz" ;;
    *)       PIPER_TAR="" ;;
esac
if [ -n "$PIPER_TAR" ] && ! command -v piper >/dev/null 2>&1; then
    mkdir -p "$DATA_DIR/piper"
    fetch "https://github.com/rhasspy/piper/releases/download/$PIPER_RELEASE/$PIPER_TAR" \
          "$DATA_DIR/piper.tar.gz"
    tar -xzf "$DATA_DIR/piper.tar.gz" -C "$DATA_DIR"
    rm -f "$DATA_DIR/piper.tar.gz"
    PIPER_HOME="$DATA_DIR/piper/piper"
else
    PIPER_HOME="$(command -v piper 2>/dev/null || true)"
    PIPER_HOME="${PIPER_HOME%/piper}"
fi

say "Downloading Piper voice $VOICE_NAME"
VOICE_DIR="$DATA_DIR/voices"
mkdir -p "$VOICE_DIR"
if [ ! -f "$VOICE_DIR/$VOICE_NAME.onnx" ]; then
    fetch "https://huggingface.co/rhasspy/piper-voices/resolve/$VOICE_REPO_PATH/$VOICE_NAME.onnx" \
          "$VOICE_DIR/$VOICE_NAME.onnx.part"
    mv "$VOICE_DIR/$VOICE_NAME.onnx.part" "$VOICE_DIR/$VOICE_NAME.onnx"
fi
if [ ! -f "$VOICE_DIR/$VOICE_NAME.onnx.json" ]; then
    fetch "https://huggingface.co/rhasspy/piper-voices/resolve/$VOICE_REPO_PATH/$VOICE_NAME.onnx.json" \
          "$VOICE_DIR/$VOICE_NAME.onnx.json.part"
    mv "$VOICE_DIR/$VOICE_NAME.onnx.json.part" "$VOICE_DIR/$VOICE_NAME.onnx.json"
fi

if [ -n "${PIPER_HOME:-}" ] && [ -x "$PIPER_HOME/piper" ]; then
    # wrapper keeps piper's bundled espeak-ng-data reachable relative to cwd
    mkdir -p "$INSTALL_BIN"
    cat > "$INSTALL_BIN/piper" <<EOF
#!/usr/bin/env sh
cd "$PIPER_HOME"
exec ./piper "\$@"
EOF
    chmod +x "$INSTALL_BIN/piper"
fi

# ── 6. config ─────────────────────────────────────────────────────────────
if [ ! -f "$CONFIG_DIR/config.toml" ]; then
    say "Writing $CONFIG_DIR/config.toml"
    mkdir -p "$CONFIG_DIR"
    # Edit the example in place — never append a second [tts] table (TOML
    # forbids duplicate tables; that bug broke whole-file parsing).
    sed -e "s/^model = \"base\"/model = \"$MODEL\"/" \
        -e 's|^engine = "auto"|engine = "piper"|' \
        -e "s|^voice = \"\".*|voice = \"$VOICE_DIR/$VOICE_NAME.onnx\"|" \
        "$REPO_DIR/config.example.toml" > "$CONFIG_DIR/config.toml"
fi

# Heal configs written by older installers that appended a duplicate [tts]
# table: drop the FIRST occurrence, the later (piper) block wins.
if [ -f "$CONFIG_DIR/config.toml" ] && \
   [ "$(grep -c '^\[tts\]' "$CONFIG_DIR/config.toml" 2>/dev/null)" -gt 1 ]; then
    say "Removing duplicate [tts] section from config.toml (old installer bug)"
    awk '/^\[tts\]$/{c++; if(c==1){s=1; next}} s{if(/^\[/){s=0} else next} {print}' \
        "$CONFIG_DIR/config.toml" > "$CONFIG_DIR/config.toml.tmp" &&
        mv "$CONFIG_DIR/config.toml.tmp" "$CONFIG_DIR/config.toml"
fi

if [ -f "$CONFIG_DIR/config.toml" ] && ! grep -q '^\[dictation\]' "$CONFIG_DIR/config.toml"; then
    say "Adding [dictation] section to $CONFIG_DIR/config.toml"
    printf '\n[dictation]\nkey = "scrolllock"\ninjector = "auto"\nnotifications = true\n' \
        >> "$CONFIG_DIR/config.toml"
fi

if [ -f "$CONFIG_DIR/config.toml" ] && ! grep -q '^\[stt\]' "$CONFIG_DIR/config.toml"; then
    printf '\n[stt]\nengine = "whisper"\nmodel = "%s"\nlanguage = "en"\n' "$MODEL" \
        >> "$CONFIG_DIR/config.toml"
fi

mkdir -p "$CONFIG_DIR/agents"
if [ ! -f "$CONFIG_DIR/agents/example-local.toml" ]; then
    cat > "$CONFIG_DIR/agents/example-local.toml" <<'AGENT'
name = "example"
api_base = "http://localhost:8080/v1"
api_key = ""
model = "local-model"
AGENT
fi
if [ ! -f "$CONFIG_DIR/agents/ollama-local.toml" ]; then
    cat > "$CONFIG_DIR/agents/ollama-local.toml" <<'AGENT'
# Works out of the box if ollama is running: `ollama serve` + `ollama pull llama3.2`
name = "ollama-local"
api_base = "http://127.0.0.1:11434/v1"
api_key = ""
model = "llama3.2"
AGENT
fi
if [ ! -f "$CONFIG_DIR/agents/llamafile-local.toml" ]; then
    cat > "$CONFIG_DIR/agents/llamafile-local.toml" <<'AGENT'
# llamafile / llama.cpp server template: `./llamafile --port 8080`
name = "llamafile-local"
api_base = "http://127.0.0.1:8080/v1"
api_key = ""
model = "local"
AGENT
fi
if [ ! -f "$CONFIG_DIR/agents/cloud-openai-template.toml" ]; then
    cat > "$CONFIG_DIR/agents/cloud-openai-template.toml" <<'AGENT'
# Cloud template — paste a key or export VOXBOX_API_KEY before starting vbd.
name = "cloud-openai"
api_base = "https://api.openai.com/v1"
api_key = "sk-paste-yours-here"
model = "gpt-4o-mini"
AGENT
fi

# ── 7. desktop menu entry + icon + (optional) autostart ───────────────────
say "Installing menu entry and icon"
APPS_DIR="$HOME/.local/share/applications"
mkdir -p "$APPS_DIR" "$HOME/.local/share/icons/hicolor/scalable/apps" \
         "$HOME/.local/share/icons/hicolor/512x512/apps"
install -m 0644 "$REPO_DIR/assets/voxbox.svg" \
    "$HOME/.local/share/icons/hicolor/scalable/apps/voxbox.svg"
if [ -f "$REPO_DIR/assets/voxbox.png" ]; then
    install -m 0644 "$REPO_DIR/assets/voxbox.png" \
        "$HOME/.local/share/icons/hicolor/512x512/apps/voxbox.png"
fi
sed -e "s|^Exec=vbd --toggle|Exec=$INSTALL_BIN/vbd --toggle|" \
    -e "s|^Exec=vbd --window|Exec=$INSTALL_BIN/vbd --window|" \
    -e "s|^Exec=voxbox dictation|Exec=$INSTALL_BIN/voxbox dictation|" \
    -e "s|^Exec=vbd$|Exec=$INSTALL_BIN/vbd|" \
    -e "s|^Exec=voxbox$|Exec=$INSTALL_BIN/voxbox|" \
    -e "s|^Exec=voxbox --text|Exec=$INSTALL_BIN/voxbox --text|" \
    -e "s|^TryExec=vbd|TryExec=$INSTALL_BIN/vbd|" \
    "$REPO_DIR/assets/voxbox.desktop" > "$APPS_DIR/voxbox.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS_DIR" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && \
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" || true

# ONE app, ONE name: the same "VoxBox" entry is the desktop icon. Clicking
# it (menu or desktop) toggles talk — or wakes the resident if it is asleep.
# Studio stays one right-click action away and one tray-click away.
if [ -d "$HOME/Desktop" ]; then
    say "Installing the VoxBox icon on your desktop"
    install -m 0755 "$APPS_DIR/voxbox.desktop" "$HOME/Desktop/voxbox.desktop"
    command -v gio >/dev/null 2>&1 && \
        gio set "$HOME/Desktop/voxbox.desktop" metadata::trusted true 2>/dev/null || true
fi
# remove the old two-app leftovers (pre-0.4.0)
rm -f "$HOME/Desktop/voxbox-tile.desktop" "$APPS_DIR/voxbox-tile.desktop"

# ── 8. permissions needed for global hotkey + text injection ──────────────
say "Permission setup (uinput rule + group memberships)"
if command -v sudo >/dev/null 2>&1; then
    SUDO=sudo
else
    SUDO=
fi
$SUDO groupadd -f uinput 2>/dev/null || true
printf 'KERNEL=="uinput", GROUP="uinput", MODE="0660"\n' \
    | $SUDO tee /etc/udev/rules.d/60-voxbox-uinput.rules >/dev/null || true
printf 'SUBSYSTEM=="input", KERNEL=="event*", TAG+="uaccess"\n' \
    | $SUDO tee /etc/udev/rules.d/60-voxbox-input.rules >/dev/null || true
$SUDO udevadm control --reload 2>/dev/null || true
$SUDO udevadm trigger /dev/uinput 2>/dev/null || true
$SUDO udevadm trigger --subsystem-match=input 2>/dev/null || true
echo "  Add your user to both groups (activation needs a fresh login):"
echo "    sudo usermod -aG input,uinput \$USER"
echo "  Log out and back in afterwards."
echo "  (The udev rule above already grants uinput to the uinput group,"
echo "   which the groupadd created if it was missing on your distro.)"

CONFIGURED_KEY="$(sed -n 's/^key *= *"\(.*\)"$/\1/p' "$CONFIG_DIR/config.toml" 2>/dev/null | head -1)"

if [ "$WITH_PARAKEET" = "1" ]; then
  ENGINES_NOTE="parakeet model downloaded (ready in Studio)"
else
  ENGINES_NOTE="parakeet model (~2.5 GB) downloads on first use, or pre-fetch with: bash install.sh --with-parakeet"
fi

cat <<EOF

  VoxBox installed.
    Desktop icon:   ONE app — "VoxBox" in the menu AND on your desktop.
                    Click it to talk: speech is typed into the focused app
                    (clicking also wakes the resident if it is asleep).
                    Right-click → Open VoxBox Studio for voice, engines, agents.
    Studio window:  left-click the tray Being, or menu → "VoxBox" (vbd --window)
    Tray icon:      appears when the dictation resident runs (state + Record now + quit)
    Engines:        Studio → Speech engine — whisper | parakeet
                    (${ENGINES_NOTE})
    Agents:         $CONFIG_DIR/agents/*.toml — pick them in Studio → AI agents
    Voice shell:    $INSTALL_BIN/voxbox
    AutoStart:      mkdir -p ~/.config/autostart &&
                    cp $APPS_DIR/voxbox.desktop ~/.config/autostart/
    Hotkey:         ${CONFIGURED_KEY:-scrolllock}  (still available in $CONFIG_DIR/config.toml → [dictation] key)
EOF
