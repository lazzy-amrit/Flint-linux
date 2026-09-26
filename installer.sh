#!/usr/bin/env bash
# Flint installer for Linux.
# Downloads the released AppImage, installs it, and registers a desktop
# entry + a `flint` command so the launcher shows up in app menus (rofi,
# dmenu, GNOME/KDE search, etc.) instead of needing `./Flint.AppImage`.
set -euo pipefail

REPO="lazzy-amrit/Flint-linux"
VERSION="0.3.0"
APPIMAGE_NAME="Flint-${VERSION}-x86_64.AppImage"
DOWNLOAD_URL="https://raw.githubusercontent.com/${REPO}/main/Releases/${VERSION}/${APPIMAGE_NAME}"
ICON_URL="https://raw.githubusercontent.com/${REPO}/main/src-tauri/icons/icon.png"

INSTALL_DIR="$HOME/.local/share/flint"
BIN_DIR="$HOME/.local/bin"
DESKTOP_DIR="$HOME/.local/share/applications"

echo "==> Installing Flint ${VERSION}"

mkdir -p "$INSTALL_DIR" "$BIN_DIR" "$DESKTOP_DIR"

echo "==> Downloading AppImage..."
curl -fL --progress-bar -o "$INSTALL_DIR/Flint.AppImage" "$DOWNLOAD_URL"
chmod +x "$INSTALL_DIR/Flint.AppImage"

echo "==> Downloading icon..."
curl -fsSL -o "$INSTALL_DIR/icon.png" "$ICON_URL" || echo "warning: icon download failed, continuing without one"

# Confirm the AppImage can actually run: it needs libfuse2/fuse3 to mount
# itself. Modern AppImage runtimes fall back to extract-and-run automatically
# if FUSE is missing, but that's slower and not guaranteed on every distro,
# so we make sure it's present up front instead of surprising the user.
if ! ldconfig -p 2>/dev/null | grep -qi "libfuse"; then
    echo "==> libfuse not found."
    if command -v pacman >/dev/null 2>&1; then
        echo "==> Installing fuse2 via pacman (needs sudo)..."
        sudo pacman -S --needed --noconfirm fuse2
    else
        echo "    Please install libfuse2 (or fuse3) using your distro's package manager,"
        echo "    e.g. 'apt install libfuse2' on Debian/Ubuntu, then re-run this script."
    fi
fi

echo "==> Creating 'flint' command..."
cat > "$BIN_DIR/flint" << EOF
#!/usr/bin/env bash
exec "$INSTALL_DIR/Flint.AppImage" "\$@"
EOF
chmod +x "$BIN_DIR/flint"

echo "==> Registering desktop entry..."
cat > "$DESKTOP_DIR/flint.desktop" << EOF
[Desktop Entry]
Type=Application
Name=Flint
Comment=Minecraft launcher
Exec=$BIN_DIR/flint %U
Icon=$INSTALL_DIR/icon.png
Terminal=false
Categories=Game;
StartupWMClass=flint
EOF

if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database "$DESKTOP_DIR" >/dev/null 2>&1 || true
fi

if ! echo "$PATH" | tr ':' '\n' | grep -qx "$BIN_DIR"; then
    echo
    echo "NOTE: $BIN_DIR is not on your PATH."
    echo "Add this to your shell config (~/.bashrc or ~/.zshrc) and restart your terminal:"
    echo "    export PATH=\"\$HOME/.local/bin:\$PATH\""
fi

echo
echo "==> Done. Launch Flint by typing 'flint', or find it in your app launcher/rofi."
