#!/usr/bin/env bash
# Build the Rust core for Android and generate its Kotlin bindings.
# Needs: the Android NDK (ANDROID_NDK_HOME), cargo-ndk, Rust Android targets.
#   rustup target add aarch64-linux-android x86_64-linux-android
#   cargo install cargo-ndk
set -euo pipefail
cd "$(dirname "$0")"
APP=app/src/main
ROOT=../..

# Native library for phones (arm64) and emulators (x86_64).
(cd "$ROOT" && cargo ndk -t arm64-v8a -t x86_64 -P 26 -o "apps/android/$APP/jniLibs" build --release -p veena-ffi)

# Kotlin bindings, generated from the library.
(cd "$ROOT" && cargo run -q -p veena-ffi --bin uniffi-bindgen -- generate \
    --library "apps/android/$APP/jniLibs/arm64-v8a/libveena_ffi.so" \
    --language kotlin --no-format --out-dir "apps/android/$APP/java")

# Bricks shipped inside the app; installed on first launch.
mkdir -p "$APP/assets/plugins"
cp "$ROOT"/plugins/*.js "$APP/assets/plugins/"
echo "Rust core, bindings and bricks are ready."
