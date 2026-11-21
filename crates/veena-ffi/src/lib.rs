//! The Veena core for the Android app (and any other front end).
//!
//! Exposes one [`Veena`] object through UniFFI; Kotlin bindings are
//! generated from this crate. All calls are thread-safe. Calls that wait
//! for the network (`search`, `update_plugins`) block, so call them off the
//! UI thread.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use veena_core::{Repeat, SourceRegistry, Track, TrackId, UrlSource};
use veena_player::{PlaybackState, Player, PlayerEvent, Snapshot};
use veena_plugin::Options;

uniffi::setup_scaffolding!();

/// Errors arrive in Kotlin as `VeenaException` with this text as `message`.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum VeenaError {
    #[error("{message}")]
    Failed { message: String },
}

impl VeenaError {
    fn new(message: impl ToString) -> Self {
        Self::Failed {
            message: message.to_string(),
        }
    }
}

/// A track as the interface shows it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TrackInfo {
    /// Content kind, e.g. `youtube`.
    pub kind: String,
    /// Id within the kind.
    pub id: String,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<u64>,
    pub artwork: Option<String>,
    /// Name of the catalog that found it, e.g. `YouTube Music`.
    pub source: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PlayState {
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

/// Everything the player screen needs.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PlayerState {
    pub state: PlayState,
    pub queue: Vec<TrackInfo>,
    pub current: Option<u32>,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: RepeatMode,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub search: bool,
    pub audio_links: bool,
    /// Why it did not load, if it did not.
    pub error: Option<String>,
}

/// Receives player updates. Called from a background thread.
#[uniffi::export(callback_interface)]
pub trait PlayerListener: Send + Sync {
    fn on_state(&self, state: PlayerState);
    fn on_error(&self, message: String);
}

#[derive(uniffi::Object)]
pub struct Veena {
    runtime: tokio::runtime::Runtime,
    registry: SourceRegistry,
    player: Player,
    plugins_dir: PathBuf,
    plugins: Mutex<Vec<PluginInfo>>,
    /// Source names by track (kind, id), to label tracks in the queue.
    sources: Mutex<std::collections::HashMap<(String, String), String>>,
}

#[uniffi::export]
impl Veena {
    /// Start the core. Plugins are loaded from `<data_dir>/plugins`.
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Result<Arc<Self>, VeenaError> {
        init_logging();
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("veena")
            .build()
            .map_err(VeenaError::new)?;
        let plugins_dir = PathBuf::from(data_dir).join("plugins");
        std::fs::create_dir_all(&plugins_dir).map_err(VeenaError::new)?;

        let registry = SourceRegistry::new();
        // Built-in audio sources: files on the device and direct links.
        registry.add_resolver(Arc::new(veena_src_local::LocalSource::default()));
        registry.add_resolver(Arc::new(UrlSource));
        let player = runtime
            .block_on(async { Player::new(registry.clone()) })
            .map_err(VeenaError::new)?;
        let veena = Arc::new(Self {
            runtime,
            registry,
            player,
            plugins_dir,
            plugins: Mutex::default(),
            sources: Mutex::default(),
        });
        veena.reload_plugins();
        Ok(veena)
    }

    /// (Re)load every brick from the plugin folder.
    pub fn reload_plugins(&self) {
        let loaded = self.runtime.block_on(veena_updater::load_installed(
            &self.plugins_dir,
            &Options::default(),
        ));
        let mut infos = Vec::new();
        for (path, plugin) in loaded.unwrap_or_default() {
            match plugin {
                Ok(plugin) => {
                    let meta = plugin.meta();
                    infos.push(PluginInfo {
                        id: meta.id.clone(),
                        name: meta.name.clone(),
                        version: meta.version.clone(),
                        search: plugin.is_catalog(),
                        audio_links: plugin.is_resolver(),
                        error: None,
                    });
                    Arc::new(plugin).install(&self.registry);
                }
                Err(e) => infos.push(PluginInfo {
                    id: path
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    name: String::new(),
                    version: String::new(),
                    search: false,
                    audio_links: false,
                    error: Some(e.to_string()),
                }),
            }
        }
        *self.plugins.lock().unwrap() = infos;
    }

    pub fn plugins(&self) -> Vec<PluginInfo> {
        self.plugins.lock().unwrap().clone()
    }

    /// Search every catalog. Blocks; call off the UI thread.
    pub fn search(&self, query: String) -> Result<Vec<TrackInfo>, VeenaError> {
        let results = self.runtime.block_on(async {
            let mut all = Vec::new();
            let mut errors = Vec::new();
            for catalog in self.registry.catalogs() {
                match catalog.search(&query).await {
                    Ok(tracks) => {
                        all.extend(tracks.into_iter().map(|t| (catalog.name().to_owned(), t)))
                    }
                    Err(e) => errors.push(format!("{}: {e}", catalog.name())),
                }
            }
            (all, errors)
        });
        let (tracks, errors) = results;
        if tracks.is_empty() && !errors.is_empty() {
            return Err(VeenaError::new(errors.join("; ")));
        }
        let mut sources = self.sources.lock().unwrap();
        Ok(tracks
            .into_iter()
            .map(|(source, track)| {
                sources.insert((track.id.kind.clone(), track.id.id.clone()), source.clone());
                to_info(&track, source)
            })
            .collect())
    }

    /// Replace the queue with `tracks` and play from `start`.
    pub fn play(&self, tracks: Vec<TrackInfo>, start: u32) {
        self.remember_sources(&tracks);
        self.player.clear();
        self.player.enqueue(tracks.iter().map(from_info).collect());
        self.player.play_at(start as usize);
    }

    /// Add tracks to the end of the queue.
    pub fn enqueue(&self, tracks: Vec<TrackInfo>) {
        self.remember_sources(&tracks);
        self.player.enqueue(tracks.iter().map(from_info).collect());
    }

    pub fn play_at(&self, index: u32) {
        self.player.play_at(index as usize);
    }

    pub fn toggle_pause(&self) {
        self.player.toggle_pause();
    }

    pub fn play_current(&self) {
        self.player.play();
    }

    pub fn pause(&self) {
        self.player.pause();
    }

    pub fn next(&self) {
        self.player.next();
    }

    pub fn previous(&self) {
        self.player.previous();
    }

    pub fn stop(&self) {
        self.player.stop();
    }

    pub fn seek(&self, position_ms: u64) {
        self.player.seek(Duration::from_millis(position_ms));
    }

    pub fn set_volume(&self, volume: f32) {
        self.player.set_volume(volume);
    }

    pub fn set_shuffle(&self, shuffle: bool) {
        self.player.set_shuffle(shuffle);
    }

    pub fn set_repeat(&self, repeat: RepeatMode) {
        self.player.set_repeat(match repeat {
            RepeatMode::Off => Repeat::Off,
            RepeatMode::All => Repeat::All,
            RepeatMode::One => Repeat::One,
        });
    }

    pub fn state(&self) -> PlayerState {
        let snapshot = self.runtime.block_on(self.player.snapshot());
        self.to_state(snapshot)
    }

    /// Deliver every state change and error to `listener`.
    pub fn set_listener(self: Arc<Self>, listener: Box<dyn PlayerListener>) {
        let mut events = self.player.subscribe();
        let veena = self.clone();
        self.runtime.spawn(async move {
            loop {
                match events.recv().await {
                    Ok(PlayerEvent::Error(message)) => listener.on_error(message),
                    Ok(_) => {
                        let snapshot = veena.player.snapshot().await;
                        listener.on_state(veena.to_state(snapshot));
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
    }

    /// Install signed brick updates, then reload. Returns a summary.
    pub fn update_plugins(
        &self,
        manifest_url: String,
        public_key: String,
    ) -> Result<String, VeenaError> {
        let updater = veena_updater::Updater::new(&self.plugins_dir, &public_key, manifest_url)
            .map_err(VeenaError::new)?;
        let report = self
            .runtime
            .block_on(updater.check())
            .map_err(VeenaError::new)?;
        if !report.installed.is_empty() || !report.settings_changed.is_empty() {
            self.reload_plugins();
        }
        let mut lines: Vec<String> = report
            .installed
            .iter()
            .map(|(id, _, to)| format!("{id} {to}"))
            .collect();
        lines.extend(
            report
                .failed
                .iter()
                .map(|(id, e)| format!("{id} failed: {e}")),
        );
        Ok(if lines.is_empty() {
            "Everything is up to date".into()
        } else {
            lines.join("\n")
        })
    }
}

impl Veena {
    fn remember_sources(&self, tracks: &[TrackInfo]) {
        let mut sources = self.sources.lock().unwrap();
        for t in tracks.iter().filter(|t| !t.source.is_empty()) {
            sources.insert((t.kind.clone(), t.id.clone()), t.source.clone());
        }
    }

    fn to_state(&self, snapshot: Snapshot) -> PlayerState {
        let sources = self.sources.lock().unwrap();
        PlayerState {
            state: match snapshot.state {
                PlaybackState::Stopped => PlayState::Stopped,
                PlaybackState::Loading => PlayState::Loading,
                PlaybackState::Playing => PlayState::Playing,
                PlaybackState::Paused => PlayState::Paused,
            },
            queue: snapshot
                .tracks
                .iter()
                .map(|t| {
                    let source = sources
                        .get(&(t.id.kind.clone(), t.id.id.clone()))
                        .cloned()
                        .unwrap_or_default();
                    to_info(t, source)
                })
                .collect(),
            current: snapshot.current.map(|i| i as u32),
            position_ms: snapshot.position.as_millis() as u64,
            duration_ms: snapshot.duration.map(|d| d.as_millis() as u64),
            volume: snapshot.volume,
            shuffle: snapshot.shuffle,
            repeat: match snapshot.repeat {
                Repeat::Off => RepeatMode::Off,
                Repeat::All => RepeatMode::All,
                Repeat::One => RepeatMode::One,
            },
        }
    }
}

fn to_info(track: &Track, source: String) -> TrackInfo {
    TrackInfo {
        kind: track.id.kind.clone(),
        id: track.id.id.clone(),
        title: track.title.clone(),
        artist: track.artist.clone(),
        album: track.album.clone(),
        duration_ms: track.duration.map(|d| d.as_millis() as u64),
        artwork: track.artwork.clone(),
        source,
    }
}

fn from_info(info: &TrackInfo) -> Track {
    Track {
        id: TrackId::new(info.kind.clone(), info.id.clone()),
        title: info.title.clone(),
        artist: info.artist.clone(),
        album: info.album.clone(),
        duration: info.duration_ms.map(Duration::from_millis),
        artwork: info.artwork.clone(),
    }
}

fn init_logging() {
    #[cfg(target_os = "android")]
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("veena"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    struct Collect(Mutex<mpsc::Sender<PlayerState>>);

    impl PlayerListener for Collect {
        fn on_state(&self, state: PlayerState) {
            let _ = self.0.lock().unwrap().send(state);
        }
        fn on_error(&self, message: String) {
            panic!("unexpected error: {message}");
        }
    }

    /// Half a second of silence as a WAV file.
    fn wav() -> Vec<u8> {
        let (rate, samples) = (8000u32, 4000u32);
        let mut out = b"RIFF".to_vec();
        out.extend((36 + samples * 2).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * 2).to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend((samples * 2).to_le_bytes());
        out.extend(vec![0u8; samples as usize * 2]);
        out
    }

    #[test]
    fn plays_a_file_and_reports_state() {
        let dir = std::env::temp_dir().join(format!("veena-ffi-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("song.wav");
        std::fs::write(&file, wav()).unwrap();

        let veena = match Veena::new(dir.to_string_lossy().into()) {
            Ok(veena) => veena,
            // No audio device on this machine (e.g. CI without ALSA config).
            Err(e) => return eprintln!("skipping: {e}"),
        };
        assert!(veena.plugins().is_empty());
        assert_eq!(veena.state().state, PlayState::Stopped);
        assert!(veena.search("anything".into()).unwrap().is_empty());

        let (tx, rx) = mpsc::channel();
        veena
            .clone()
            .set_listener(Box::new(Collect(Mutex::new(tx))));
        let track = TrackInfo {
            kind: "file".into(),
            id: file.to_string_lossy().into(),
            title: "Song".into(),
            artist: None,
            album: None,
            duration_ms: None,
            artwork: None,
            source: "Local files".into(),
        };
        veena.play(vec![track.clone()], 0);

        let mut saw_playing = false;
        while let Ok(state) = rx.recv_timeout(Duration::from_secs(10)) {
            assert_eq!(state.queue, vec![track.clone()]);
            saw_playing |= state.state == PlayState::Playing;
            if saw_playing && state.state == PlayState::Stopped {
                break;
            }
        }
        assert!(saw_playing, "playback started");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
