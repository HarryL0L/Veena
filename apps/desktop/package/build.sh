#!/bin/sh
# Build the Linux download for this machine's architecture:
# dist/veena-music-linux-<arch>.tar.gz (x86_64 or aarch64).
# Run from the repository root.
set -eu
cargo build --release --locked -p veena-desktop -p veena
name="veena-music-linux-$(uname -m)"
out="dist/$name"
rm -rf "$out"
mkdir -p "$out"
cp target/release/veena-desktop target/release/veena "$out/"
strip "$out/veena-desktop" "$out/veena" 2>/dev/null || true
cp apps/desktop/veena-desktop.desktop apps/desktop/data/veena-music.svg "$out/"
cp apps/desktop/package/install.sh apps/desktop/package/uninstall.sh apps/desktop/package/README.txt "$out/"
tar -C dist -czf "dist/$name.tar.gz" "$name"
ls -l "dist/$name.tar.gz"
