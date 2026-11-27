//! The Veena player: queue, sources and audio engine working together.
//!
//! [`Player`] is the single entry point a user interface needs. Commands are
//! fire-and-forget; state changes arrive as [`PlayerEvent`]s.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{broadcast, mpsc, oneshot};
use veena_audio::{AudioEngine, AudioEvent};
use veena_core::{Queue, Repeat, SourceRegistry, Stream, StreamLocation, Track};

pub use veena_audio::{AudioCache, AudioError, PlaybackState};

/// "Previous" restarts the current track if it has played longer than this.
const RESTART_THRESHOLD: Duration = Duration::from_secs(3);

/// Something the interface may want to show.
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerEvent {
    /// A new track became current. `index` points into the queue.
    TrackChanged {
        index: usize,
        track: Track,
    },
    StateChanged(PlaybackState),
    Position {
        position: Duration,
        duration: Option<Duration>,
    },
    /// Tracks were added, removed or reordered, or shuffle/repeat changed.
    QueueChanged,
    /// Playback reached the end of the queue (repeat off), or no track in
    /// the queue could be played.
    QueueEnded,
    Error(String),
}

/// The full player state at one moment.
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub tracks: Vec<Track>,
    pub current: Option<usize>,
    pub state: PlaybackState,
    pub position: Duration,
    pub duration: Option<Duration>,
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
    /// Playback pauses when the current track ends (a sleep timer).
    pub pause_at_end: bool,
}

enum Message {
    Enqueue(Vec<Track>),
    PlayNext(Track),
    Remove(usize),
    Move(usize, usize),
    Clear,
    PlayAt(usize),
    TogglePause,
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    Seek(Duration),
    SeekBy(i64),
    SetVolume(f32),
    SetShuffle(bool),
    SetRepeat(Repeat),
    SetPauseAtEnd(bool),
    Snapshot(oneshot::Sender<Snapshot>),
    Shutdown,
    // Internal
    Resolved {
        generation: u64,
        result: Result<Stream, String>,
    },
    Audio(AudioEvent),
}

/// Handle to a running player. Dropping it stops playback.
pub struct Player {
    messages: mpsc::UnboundedSender<Message>,
    events: broadcast::Sender<PlayerEvent>,
}

impl Player {
    /// Open the audio output and start the player.
    ///
    /// Must be called from within a Tokio runtime.
    pub fn new(registry: SourceRegistry) -> Result<Self, AudioError> {
        Self::with_cache(registry, None)
    }

    /// Like [`Player::new`], keeping songs in `cache` as they stream and
    /// playing from it when it has them.
    ///
    /// Must be called from within a Tokio runtime.
    pub fn with_cache(
        registry: SourceRegistry,
        cache: Option<Arc<AudioCache>>,
    ) -> Result<Self, AudioError> {
        let (messages, inbox) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(256);

        let audio_tx = messages.clone();
        let engine = AudioEngine::new(move |event| {
            let _ = audio_tx.send(Message::Audio(event));
        })?;

        let actor = Actor {
            registry,
            cache,
            queue: Queue::new(),
            engine,
            events: events.clone(),
            loopback: messages.clone(),
            generation: 0,
            resolving: false,
            awaiting_audio: false,
            autoplay: true,
            pause_at_end: false,
            failures: 0,
            state: PlaybackState::Stopped,
            position: Duration::ZERO,
            duration: None,
            volume: 1.0,
        };
        tokio::spawn(actor.run(inbox));
        Ok(Self { messages, events })
    }

    /// Receive player events. Slow receivers may miss position updates.
    pub fn subscribe(&self) -> broadcast::Receiver<PlayerEvent> {
        self.events.subscribe()
    }

    /// Add tracks to the end of the queue.
    pub fn enqueue(&self, tracks: Vec<Track>) {
        self.send(Message::Enqueue(tracks));
    }

    /// Insert a track right after the current one.
    pub fn play_next(&self, track: Track) {
        self.send(Message::PlayNext(track));
    }

    pub fn remove(&self, index: usize) {
        self.send(Message::Remove(index));
    }

    /// Move a queue entry; the current track keeps playing.
    pub fn move_track(&self, from: usize, to: usize) {
        self.send(Message::Move(from, to));
    }

    pub fn clear(&self) {
        self.send(Message::Clear);
    }

    /// Play the queue entry at `index`.
    pub fn play_at(&self, index: usize) {
        self.send(Message::PlayAt(index));
    }

    /// Pause if playing; otherwise play (starting the queue if needed).
    pub fn toggle_pause(&self) {
        self.send(Message::TogglePause);
    }

    /// Play (starting the queue if needed). No effect if already playing.
    pub fn play(&self) {
        self.send(Message::Play);
    }

    /// Pause. No effect unless playing or loading.
    pub fn pause(&self) {
        self.send(Message::Pause);
    }

    pub fn stop(&self) {
        self.send(Message::Stop);
    }

    pub fn next(&self) {
        self.send(Message::Next);
    }

    /// Restart the current track, or go to the previous one near its start.
    pub fn previous(&self) {
        self.send(Message::Previous);
    }

    pub fn seek(&self, position: Duration) {
        self.send(Message::Seek(position));
    }

    /// Seek relative to the current position, in milliseconds.
    pub fn seek_by(&self, delta_ms: i64) {
        self.send(Message::SeekBy(delta_ms));
    }

    /// Volume from 0.0 to 1.0.
    pub fn set_volume(&self, volume: f32) {
        self.send(Message::SetVolume(volume));
    }

    pub fn set_shuffle(&self, shuffle: bool) {
        self.send(Message::SetShuffle(shuffle));
    }

    pub fn set_repeat(&self, repeat: Repeat) {
        self.send(Message::SetRepeat(repeat));
    }

    /// Pause when the current track ends, as a sleep timer: the next track
    /// is loaded but waits, paused. Also works with repeat-one.
    pub fn set_pause_at_end(&self, on: bool) {
        self.send(Message::SetPauseAtEnd(on));
    }

    pub async fn snapshot(&self) -> Snapshot {
        let (tx, rx) = oneshot::channel();
        self.send(Message::Snapshot(tx));
        rx.await
            .expect("player task is running while the handle exists")
    }

    fn send(&self, message: Message) {
        let _ = self.messages.send(message);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.send(Message::Shutdown);
    }
}

/// Owns all player state; runs as a single task so no locking is needed.
struct Actor {
    cache: Option<Arc<AudioCache>>,
    registry: SourceRegistry,
    queue: Queue,
    engine: AudioEngine,
    events: broadcast::Sender<PlayerEvent>,
    loopback: mpsc::UnboundedSender<Message>,
    /// Bumped whenever a new track starts, to ignore stale resolve results.
    generation: u64,
    /// Waiting for a source to return a stream.
    resolving: bool,
    /// Waiting for the audio engine to open the stream.
    awaiting_audio: bool,
    /// Whether the track being loaded should start playing.
    autoplay: bool,
    /// Pause when the current track ends.
    pause_at_end: bool,
    /// Consecutive tracks that failed to play, to avoid skipping forever.
    failures: usize,
    state: PlaybackState,
    position: Duration,
    duration: Option<Duration>,
    volume: f32,
}

impl Actor {
    async fn run(mut self, mut inbox: mpsc::UnboundedReceiver<Message>) {
        while let Some(message) = inbox.recv().await {
            if matches!(message, Message::Shutdown) {
                break;
            }
            self.handle(message);
        }
        self.engine.stop();
    }

    fn handle(&mut self, message: Message) {
        match message {
            Message::Enqueue(tracks) => {
                self.queue.extend(tracks);
                self.emit(PlayerEvent::QueueChanged);
            }
            Message::PlayNext(track) => {
                self.queue.insert_next(track);
                self.emit(PlayerEvent::QueueChanged);
            }
            Message::Move(from, to) => {
                self.queue.move_track(from, to);
                self.emit(PlayerEvent::QueueChanged);
            }
            Message::Remove(index) => {
                let was_current = self.queue.current_index() == Some(index);
                if self.queue.remove(index).is_some() {
                    self.emit(PlayerEvent::QueueChanged);
                    if was_current {
                        if self.queue.current().is_some() {
                            self.start_current();
                        } else {
                            self.halt();
                        }
                    }
                }
            }
            Message::Clear => {
                self.queue.clear();
                self.halt();
                self.emit(PlayerEvent::QueueChanged);
            }
            Message::PlayAt(index) => {
                if self.queue.jump(index).is_some() {
                    self.autoplay = true;
                    self.start_current();
                }
            }
            Message::TogglePause => match self.state {
                PlaybackState::Playing => self.engine.pause(),
                PlaybackState::Paused => self.engine.play(),
                PlaybackState::Loading => self.set_autoplay(!self.autoplay),
                PlaybackState::Stopped => {
                    self.autoplay = true;
                    if self.queue.current().is_some() {
                        self.start_current();
                    } else {
                        self.advance(false);
                    }
                }
            },
            Message::Play => match self.state {
                PlaybackState::Playing => {}
                PlaybackState::Paused => self.engine.play(),
                PlaybackState::Loading => self.set_autoplay(true),
                PlaybackState::Stopped => {
                    self.autoplay = true;
                    if self.queue.current().is_some() {
                        self.start_current();
                    } else {
                        self.advance(false);
                    }
                }
            },
            Message::Pause => match self.state {
                PlaybackState::Playing => self.engine.pause(),
                PlaybackState::Loading => self.set_autoplay(false),
                PlaybackState::Paused | PlaybackState::Stopped => {}
            },
            Message::Stop => self.halt(),
            Message::Next => self.advance(true),
            Message::Previous => {
                if self.position > RESTART_THRESHOLD && !self.resolving {
                    self.engine.seek(Duration::ZERO);
                } else if self.queue.previous().is_some() {
                    self.start_current();
                }
            }
            Message::Seek(position) => self.engine.seek(position),
            Message::SeekBy(delta_ms) => {
                let target = if delta_ms >= 0 {
                    self.position + Duration::from_millis(delta_ms as u64)
                } else {
                    self.position
                        .saturating_sub(Duration::from_millis(delta_ms.unsigned_abs()))
                };
                self.engine.seek(target);
            }
            Message::SetVolume(volume) => {
                self.volume = volume.clamp(0.0, 1.0);
                self.engine.set_volume(self.volume);
            }
            Message::SetShuffle(shuffle) => {
                self.queue.set_shuffle(shuffle);
                self.emit(PlayerEvent::QueueChanged);
            }
            Message::SetRepeat(repeat) => {
                self.queue.set_repeat(repeat);
                self.emit(PlayerEvent::QueueChanged);
            }
            Message::SetPauseAtEnd(on) => {
                self.pause_at_end = on;
                self.emit(PlayerEvent::QueueChanged);
            }
            Message::Snapshot(reply) => {
                let _ = reply.send(Snapshot {
                    tracks: self.queue.tracks().to_vec(),
                    current: self.queue.current_index(),
                    state: self.state,
                    position: self.position,
                    duration: self.duration,
                    volume: self.volume,
                    shuffle: self.queue.shuffle(),
                    repeat: self.queue.repeat(),
                    pause_at_end: self.pause_at_end,
                });
            }
            Message::Resolved { generation, result } => {
                if generation != self.generation {
                    return;
                }
                self.resolving = false;
                match result {
                    Ok(stream) => {
                        self.awaiting_audio = true;
                        // Keep network songs for offline listening.
                        let writer = match (&self.cache, &stream.location, self.queue.current()) {
                            (Some(cache), StreamLocation::Http { .. }, Some(track)) => {
                                cache.writer(track, stream.mime.clone())
                            }
                            _ => None,
                        };
                        self.engine.load(stream, self.autoplay, writer);
                    }
                    Err(e) => self.track_failed(e),
                }
            }
            Message::Audio(event) => self.on_audio(event),
            Message::Shutdown => {}
        }
    }

    fn on_audio(&mut self, event: AudioEvent) {
        if self.resolving {
            return; // leftovers from the previous track
        }
        match event {
            AudioEvent::StateChanged(state @ (PlaybackState::Playing | PlaybackState::Paused)) => {
                if self.awaiting_audio {
                    self.awaiting_audio = false;
                    self.failures = 0;
                }
                self.set_state(state);
            }
            // Loading and stopping are driven by the player itself.
            AudioEvent::StateChanged(_) => {}
            AudioEvent::Position { position, duration } => {
                self.position = position;
                self.duration = duration.or(self.queue.current().and_then(|t| t.duration));
                self.emit(PlayerEvent::Position {
                    position,
                    duration: self.duration,
                });
            }
            AudioEvent::TrackEnded => {
                if self.pause_at_end {
                    // The sleep timer: the next track loads, paused.
                    self.pause_at_end = false;
                    self.autoplay = false;
                }
                self.advance(false);
            }
            AudioEvent::Error(e) if self.awaiting_audio => {
                self.awaiting_audio = false;
                self.track_failed(e);
            }
            AudioEvent::Error(e) => self.emit(PlayerEvent::Error(e)),
        }
    }

    /// Move to the next track. At the end of the queue, automatic advances
    /// stop playback; a manual "next" is ignored.
    /// Whether the track being loaded should play when ready. Once it has
    /// been handed to the audio engine, the engine is told too, or a pause
    /// in those moments would be lost.
    fn set_autoplay(&mut self, play: bool) {
        self.autoplay = play;
        if self.awaiting_audio {
            if play {
                self.engine.play();
            } else {
                self.engine.pause();
            }
        }
    }

    fn advance(&mut self, manual: bool) {
        if self.queue.next(manual).is_some() {
            self.start_current();
        } else if !manual {
            self.halt();
            self.emit(PlayerEvent::QueueEnded);
        }
    }

    /// Resolve and load the queue's current track.
    fn start_current(&mut self) {
        let (Some(index), Some(track)) =
            (self.queue.current_index(), self.queue.current().cloned())
        else {
            return;
        };
        self.generation += 1;
        self.resolving = true;
        self.awaiting_audio = false;
        self.position = Duration::ZERO;
        self.duration = track.duration;
        self.engine.stop();
        self.emit(PlayerEvent::TrackChanged {
            index,
            track: track.clone(),
        });
        self.set_state(PlaybackState::Loading);

        let loopback = self.loopback.clone();
        let generation = self.generation;
        if let Some(cache) = &self.cache {
            // Saved songs play from the cache, with or without the internet.
            if let Some(stream) = cache.lookup(&track) {
                let _ = loopback.send(Message::Resolved {
                    generation,
                    result: Ok(stream),
                });
                return;
            }
            if cache.offline() {
                let _ = loopback.send(Message::Resolved {
                    generation,
                    result: Err("not saved for offline listening".into()),
                });
                return;
            }
        }
        let registry = self.registry.clone();
        tokio::spawn(async move {
            let result = registry
                .resolve(&track)
                .await
                .map(|mut streams| streams.swap_remove(0))
                .map_err(|e| e.to_string());
            let _ = loopback.send(Message::Resolved { generation, result });
        });
    }

    /// Report a track that could not be played and skip to the next one,
    /// unless every track in the queue has failed in a row.
    fn track_failed(&mut self, error: String) {
        let title = self.queue.current().map_or("track", |t| t.title.as_str());
        self.emit(PlayerEvent::Error(format!("cannot play {title}: {error}")));
        self.failures += 1;
        if self.failures >= self.queue.len() {
            // Nothing in the queue is playable.
            self.failures = 0;
            self.halt();
            self.emit(PlayerEvent::QueueEnded);
        } else {
            self.advance(false);
        }
    }

    /// Stop playback and cancel any pending load.
    fn halt(&mut self) {
        self.generation += 1;
        self.resolving = false;
        self.awaiting_audio = false;
        self.position = Duration::ZERO;
        self.engine.stop();
        self.set_state(PlaybackState::Stopped);
    }

    fn set_state(&mut self, state: PlaybackState) {
        if state != self.state {
            self.state = state;
            self.emit(PlayerEvent::StateChanged(state));
        }
    }

    fn emit(&self, event: PlayerEvent) {
        let _ = self.events.send(event);
    }
}
