#!/bin/sh
# Remove Veena Music. Your liked songs, history and kept songs stay in
# ~/.local/share/veena and ~/.cache/veena; delete those too for a clean slate.
set -eu
data="${XDG_DATA_HOME:-$HOME/.local/share}"
bin="${XDG_BIN_HOME:-$HOME/.local/bin}"
rm -f "$bin/veena-desktop" "$bin/veena" \
  "$data/applications/veena-desktop.desktop" \
  "$data/icons/hicolor/scalable/apps/veena-music.svg"
update-desktop-database "$data/applications" 2>/dev/null || true
echo "Veena Music is removed."
