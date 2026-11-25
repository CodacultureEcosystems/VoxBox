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
        espeak-ng libasound2-dev
elif command -v dnf >/dev/null 2>&1; then
    say "Installing system packages (alsa, cmake, toolchain)"
    sudo dnf install -y gcc gcc-c++ cmake clang alsa-lib-devel alsa-utils \
        espeak-ng
elif command -v pacman >/dev/null 2>&1; then
    say "Installing system packages (alsa, cmake, toolchain)"
    sudo pacman -S --noconfirm base-devel cmake clang alsa-lib alsa-utils \
        espeak-ng
else
    echo "  (skip) unknown distro: install build tools + alsa-utils + espeak-ng manually" >&2
fi

if ! command -v cargo >/dev/null 2>&1; then
    say "Installing Rust (rustup)"
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    export PATH="$HOME/.cargo/bin:$PATH"
fi

# ── 2. build (Rust only: whisper.cpp compiles into the binaries) ──────────
say "Building voxbox + vbd (release)"
(cd "$REPO_DIR" && cargo build --release)

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
    {
        sed "s/^model = \"base\"/model = \"$MODEL\"/" "$REPO_DIR/config.example.toml"
        printf '\n[tts]\nengine = "piper"\nvoice = "%s/%s.onnx"\nenabled = true\n' \
            "$VOICE_DIR" "$VOICE_NAME"
    } > "$CONFIG_DIR/config.toml"
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
sed -e "s|^Exec=voxbox dictation|Exec=$INSTALL_BIN/voxbox dictation|" \
    -e "s|^Exec=vbd$|Exec=$INSTALL_BIN/vbd|" \
    -e "s|^Exec=voxbox$|Exec=$INSTALL_BIN/voxbox|" \
    -e "s|^Exec=voxbox --text|Exec=$INSTALL_BIN/voxbox --text|" \
    -e "s|^TryExec=vbd|TryExec=$INSTALL_BIN/vbd|" \
    "$REPO_DIR/assets/voxbox.desktop" > "$APPS_DIR/voxbox.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS_DIR" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && \
    gtk-update-icon-cache -f -t "$HOME/.local/share/icons/hicolor" || true

# ── 8. permissions needed for global hotkey + text injection ──────────────
say "Permission setup"
echo "  VoxBox needs two group memberships to run system-wide:"
echo "    sudo usermod -aG input  \$USER   # read /dev/input (global hotkey)"
echo "    sudo usermod -aG uinput \$USER   # create /dev/uinput virtual keyboard"
echo "  Log out and back in afterwards."
echo "  Or paste the udev rule from the README for uinput uaccess."

CONFIGURED_KEY="$(sed -n 's/^key *= *"\(.*\)"$/\1/p' "$CONFIG_DIR/config.toml" 2>/dev/null | head -1)"

cat <<EOF

  VoxBox installed.
    Desktop menu:   "VoxBox"  (right-click: dictation resident / voice shell / text)
    Tray icon:      appears when the dictation resident runs (state + Record now + quit)
    Voice shell:    $INSTALL_BIN/voxbox
    AutoStart:      mkdir -p ~/.config/autostart &&
                    cp $APPS_DIR/voxbox.desktop ~/.config/autostart/
    Hotkey:         ${CONFIGURED_KEY:-scrolllock}  (change in $CONFIG_DIR/config.toml → [dictation] key)
EOF
