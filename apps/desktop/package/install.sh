#!/bin/sh
# Install Veena Music for this user: no root needed.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
data="${XDG_DATA_HOME:-$HOME/.local/share}"
bin="${XDG_BIN_HOME:-$HOME/.local/bin}"

mkdir -p "$bin" "$data/applications" "$data/icons/hicolor/scalable/apps"
install -m 755 "$here/veena-desktop" "$bin/veena-desktop"
install -m 755 "$here/veena" "$bin/veena"
install -m 644 "$here/veena-desktop.desktop" "$data/applications/veena-desktop.desktop"
install -m 644 "$here/veena-music.svg" "$data/icons/hicolor/scalable/apps/veena-music.svg"
update-desktop-database "$data/applications" 2>/dev/null || true
gtk-update-icon-cache -q "$data/icons/hicolor" 2>/dev/null || true

echo "Veena Music is installed. Open it from your app menu, or run: veena-desktop"
case ":$PATH:" in
  *":$bin:"*) ;;
  *) echo "Note: $bin is not on your PATH; the app menu entry still works." ;;
esac
