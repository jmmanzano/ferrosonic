#!/bin/sh
set -e

REPO="https://github.com/jmmanzano/alquife"
INSTALL_DIR="/usr/local/bin"

echo "Alquife installer"
echo "===================="

# Detect architecture
ARCH=$(uname -m)
case "$ARCH" in
    x86_64) ASSET="alquife-linux-x86_64" ;;
    *)
        echo "No precompiled binary for $ARCH. Please build from source."
        echo "See: $REPO#manual-build"
        exit 1
        ;;
esac

# Detect package manager and install runtime dependencies
if command -v pacman >/dev/null 2>&1; then
    echo "Detected Arch Linux"
    sudo pacman -S --needed --noconfirm ffmpeg pipewire wireplumber dbus
elif command -v dnf >/dev/null 2>&1; then
    echo "Detected Fedora"
    sudo dnf install -y ffmpeg pipewire wireplumber dbus
elif command -v apt >/dev/null 2>&1; then
    echo "Detected Debian/Ubuntu"
    sudo apt update
    sudo apt install -y ffmpeg pipewire wireplumber libdbus-1-3
else
    echo "Unknown package manager. Please install manually: ffmpeg, pipewire, wireplumber, dbus"
    echo "Then re-run this script."
    exit 1
fi

# Optional cava install
echo ""
echo "Optional: cava is an audio visualizer that alquife can display"
echo "alongside the now-playing bar. It is not required but adds a nice"
echo "visual element that changes color with your selected theme."
echo ""
printf "Install cava? [y/N] "
read -r answer </dev/tty
if [ "$answer" = "y" ] || [ "$answer" = "Y" ]; then
    if command -v pacman >/dev/null 2>&1; then
        sudo pacman -S --needed --noconfirm cava
    elif command -v dnf >/dev/null 2>&1; then
        sudo dnf install -y cava
    elif command -v apt >/dev/null 2>&1; then
        sudo apt install -y cava
    else
        echo "Could not install cava automatically. Install it manually from: https://github.com/karlstav/cava"
    fi
    echo "cava installed. Enable it in alquife under Settings (F5)."
else
    echo "Skipping cava. You can install it later and enable it in Settings (F5)."
fi

# Download latest release binary
echo "Downloading alquife..."
LATEST=$(curl -sI "$REPO/releases/latest" | grep -i '^location:' | sed 's/.*tag\///' | tr -d '\r')
DOWNLOAD_URL="$REPO/releases/download/$LATEST/$ASSET"
TMPFILE=$(mktemp)
curl -sL "$DOWNLOAD_URL" -o "$TMPFILE"
chmod +x "$TMPFILE"

# Install
sudo mv "$TMPFILE" "$INSTALL_DIR/alquife"

echo ""
echo "Alquife $LATEST installed to $INSTALL_DIR/alquife"
echo "Run 'alquife' to start."
