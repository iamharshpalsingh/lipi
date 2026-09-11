#!/bin/sh
# Installs LiPi for the current user: copies ./lipi to ~/.local/bin (or to
# $LIPI_INSTALL_DIR) and installs the VS Code extension if VS Code is there.
# Run it from the unpacked LiPi folder:  ./install.sh
set -e

here=$(cd "$(dirname "$0")" && pwd)
dest="${LIPI_INSTALL_DIR:-$HOME/.local/bin}"

if [ ! -f "$here/lipi" ]; then
    echo "lipi wasn't found next to this script. Run install.sh from the unpacked LiPi folder."
    exit 1
fi

mkdir -p "$dest"
cp "$here/lipi" "$dest/lipi"
chmod +x "$dest/lipi"
# macOS marks downloaded programs as quarantined; this is a program you chose to install.
xattr -d com.apple.quarantine "$dest/lipi" 2>/dev/null || true

if command -v code >/dev/null 2>&1; then
    for vsix in "$here"/lipi-*.vsix; do
        if [ -f "$vsix" ]; then
            code --install-extension "$vsix" >/dev/null && echo "Installed the LiPi extension for VS Code."
        fi
    done
fi

"$dest/lipi" --version
echo "LiPi is installed in $dest"
case ":$PATH:" in
    *":$dest:"*) echo "Try:  lipi new my-app" ;;
    *)
        echo "Add $dest to your PATH, for example:"
        echo "    echo 'export PATH=\"$dest:\$PATH\"' >> ~/.profile"
        echo "then open a new terminal and try:  lipi new my-app"
        ;;
esac
