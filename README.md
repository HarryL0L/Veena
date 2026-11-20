# Veena

A modular music player for Android and Linux.

Veena keeps the player stable and makes music sources swappable. Sources are small
plugins that update on their own, so changes on a streaming site never require a
new app release.

---

## Overview

| | |
|---|---|
| **Platforms** | Android 8.0+ (API 26) · Linux |
| **Core** | Rust |
| **Android UI** | Jetpack Compose · Material 3 Expressive |
| **Linux UI** | Slint |
| **Sources** | YouTube Music · Local files · Extensible via plugins |
| **Status** | Early development · Linux terminal player available |

---

## Principles

**Stable core.** Playback, queue, library and updates live in one Rust core shared by every platform.

**Swappable sources.** Every music source is a plugin. Add, remove or replace sources without touching the core.

**Self-updating.** Plugins update in the background. No app update needed when a source changes.

**Secure by default.** Every plugin is signature-verified before it runs, sandboxed while it runs, and rolled back if it fails.

---

## How it works

```
  ┌────────────────────────────────────────────┐
  │  Interface                                 │
  │  Compose (Android)  ·  Slint (Linux)       │
  └─────────────────────┬──────────────────────┘
                        │
  ┌─────────────────────┴──────────────────────┐
  │  Veena Core  (Rust)                        │
  │  Playback · Queue · Library · Updater      │
  └─────────────────────┬──────────────────────┘
                        │
  ┌─────────────────────┴──────────────────────┐
  │  Plugins  (signed, auto-updating)          │
  │  YouTube Music  ·  Local files  ·  …       │
  └────────────────────────────────────────────┘
```

1. **The interface** shows your music and sends commands such as play, pause and search.
2. **The core** plays audio, manages the queue and keeps plugins up to date.
3. **Plugins** find music and return something playable.

---

## Design

### Android
- Material 3 Expressive with dynamic color from your wallpaper on Android 12+.
- A built-in Veena palette on Android 8–11.
- Background playback with lock screen and notification controls.

### Linux
- Native desktop app styled to match.
- Media key and desktop integration via MPRIS.

---

## Plugins

Plugins are small, signed JavaScript files run by an embedded engine inside the core.

| Step | What happens |
|---|---|
| Check | Veena checks for new plugin versions daily. |
| Verify | Each download is checked against Veena's signing key. |
| Swap | The new version loads instantly. |
| Recover | If it fails, Veena returns to the previous version. |

Plugins can only reach the domains they declare. They have no file system access.

On Linux, `yt-dlp` is available as an optional fallback and stays up to date automatically.

---

## Getting started

Veena currently runs as a terminal player on Linux.

**Requirements**

- Rust 1.88 or newer
- ALSA development files: `sudo apt install libasound2-dev pkg-config` (Debian, Ubuntu)

**Build**

```sh
cargo build --release
```

**Use**

```sh
veena play ~/Music                      # play a folder
veena play song.flac https://…/a.mp3    # files and links
veena play ~/Music --shuffle --repeat all
veena list ~/Music                      # show your library
veena search "raga" --in ~/Music        # find tracks
```

| Key | Action |
|---|---|
| `Space` | Play / pause |
| `←` `→` | Seek 5 seconds |
| `n` `b` | Next / previous |
| `↑` `↓` | Volume |
| `s` `r` | Shuffle / repeat |
| `l` | Show queue |
| `q` | Quit |

Supported formats: MP3, FLAC, AAC/M4A, ALAC, Ogg Vorbis, WAV, AIFF, MKA.

---

## Project structure

```
veena/
├── crates/
│   ├── veena-core/        Tracks, sources, queue
│   ├── veena-audio/       Decoding, streaming, output
│   ├── veena-player/      Queue and audio working together
│   └── veena-src-local/   Local music files
└── apps/
    └── linux/             Terminal player
```

Planned: `apps/android` (Compose via UniFFI), a Slint desktop app, the plugin engine and updater.

---

## Roadmap

- [x] Core playback for local files and streams
- [ ] Plugin engine and sandbox
- [ ] YouTube Music plugin
- [ ] Automatic, signed plugin updates
- [ ] Android app with Material 3 Expressive
- [ ] Linux app
- [ ] Offline downloads, lyrics, gapless playback

---

## Note

Veena is an independent project, not affiliated with YouTube or Google.
Use of third-party services is subject to their terms.
