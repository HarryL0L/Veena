# Veena Music

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
| **Status** | Early development · Linux terminal player with plugins |

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
- Background playback with lock screen, notification and headphone controls.
- Full player with large artwork, seek bar, shuffle, repeat and queue.
- Pauses for calls and when headphones are unplugged.
- Colours taken from each song's artwork; swipe covers to change songs.
- Home, Search and Library tabs: recently played, liked songs, playlists.
- Synced lyrics, queue reordering, sleep timer.

### Linux
- Desktop app in Slint, Material You styled: navigation rail, song tables,
  a Now Playing panel with live lyrics, and a player bar with volume.
- Colours from each song's artwork; X11 and Wayland.
- Right-click menus and keyboard shortcuts: Space play/pause, Ctrl+F search,
  ←/→ seek, Ctrl+←/→ previous/next, Ctrl+↑/↓ volume.
- Same core as Android: bricks, liked songs, offline cache, lyrics.
- Media keys and the desktop's media controls (GNOME, KDE and others) via
  MPRIS: song, artwork, play/pause, next/previous, seeking, shuffle, repeat
  and volume.

Run it with `cargo run --release -p veena-desktop`. To add it to your app
menu, copy `apps/desktop/veena-desktop.desktop` to `~/.local/share/applications/`,
`apps/desktop/data/veena-music.svg` to `~/.local/share/icons/hicolor/scalable/apps/`,
and the binary to a folder on your `PATH`. The UI is made with
[Slint](https://slint.dev) under its royalty-free license.

---

## Plugins

The app is one stable piece. Only the parts that need constant updates are plugins: small JavaScript files, called bricks, run by an engine built into the core. The same brick runs on Android and Linux.

| Slot | Job | Fallback |
|---|---|---|
| **Search** | Find songs, albums and artists | Every search brick is queried |
| **Audio links** | Turn a song into a playable link | If one brick fails, the next is tried |

Plugins update automatically and securely:

| Step | What happens |
|---|---|
| Check | Veena checks for new plugin versions daily. |
| Verify | Each download is checked against Veena's signing key. |
| Swap | The new version loads instantly. |
| Recover | If it fails, Veena returns to the previous version. |

Plugins can only reach the domains they declare. They have no file system access.

On Linux, `yt-dlp` will be available as an optional fallback that stays up to date automatically.

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

**Plugins**

```sh
veena --plugins ./plugins plugins                       # show installed plugins
veena --plugins ./plugins find "sitar" --play           # search plugins and play
veena --plugins ./plugins open youtube-music <album-id> # open an album
```

Plugins load from `~/.local/share/veena/plugins` by default. See [Writing a Veena plugin](plugins/README.md).

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
│   ├── veena-plugin/      JavaScript plugin engine and sandbox
│   ├── veena-updater/     Signed plugin updates and rollback
│   ├── veena-net/         Shared HTTP and TLS setup
│   ├── veena-src-local/   Local music files
│   └── veena-ffi/         Bridge from the core to Kotlin (UniFFI)
├── apps/
│   ├── android/           Android app (Jetpack Compose)
│   ├── desktop/           Linux desktop app (Slint)
│   ├── linux/             Terminal player
│   └── sign/              veena-sign: sign and publish plugin updates
└── plugins/               Source plugins and the plugin guide
```



---

## Roadmap

- [x] Core playback for local files and streams
- [x] Plugin engine and sandbox
- [x] Search and audio-link slots with automatic fallback
- [x] YouTube Music search and playback (most tracks)
- [x] Automatic, signed plugin updates
- [x] Android app with Material 3 Expressive
- [x] Background playback, lock screen and headphone controls
- [x] Official YouTube Music tracks on Android (proof-of-origin tokens)
- [x] Linux desktop app (Slint, X11 and Wayland)
- [x] Liked songs, recently played and playlists
- [x] Synced lyrics (LRCLIB brick)
- [ ] Offline downloads, gapless playback

---

## Security

Dependencies are checked for known vulnerabilities on every change and every week, and updates arrive as automatic pull requests.
See [SECURITY.md](SECURITY.md) to report a problem.

---

## Note

Veena is an independent project, not affiliated with YouTube or Google.
Use of third-party services is subject to their terms.
