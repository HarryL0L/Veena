use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};
use veena_core::Stream;

use crate::decode::{self, Input};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::{AudioError, CacheWriter};

/// How often the engine checks for the end of a track.
const TICK: Duration = Duration::from_millis(50);
/// How often position updates are reported while playing.
const POSITION_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackState {
    Stopped,
    Loading,
    Playing,
    Paused,
}

/// Notifications from the engine. Delivered on the engine's thread, so the
/// handler should be quick (typically: forward into a channel).
#[derive(Clone, Debug, PartialEq)]
pub enum AudioEvent {
    StateChanged(PlaybackState),
    Position {
        position: Duration,
        duration: Option<Duration>,
    },
    /// The current track played to the end.
    TrackEnded,
    Error(String),
}

enum Command {
    Load {
        stream: Stream,
        autoplay: bool,
        cache: Option<Box<CacheWriter>>,
    },
    Loaded {
        generation: u64,
        autoplay: bool,
        result: Result<Decoder<Input>, AudioError>,
    },
    /// The current stream, opened again after the output was replaced.
    Reopened {
        generation: u64,
        position: Duration,
        playing: bool,
        result: Result<Decoder<Input>, AudioError>,
    },
    Play,
    Pause,
    Stop,
    Seek(Duration),
    SetVolume(f32),
    Shutdown,
}

/// Plays one stream at a time on the default audio output.
///
/// Controls are non-blocking: each method sends a command to a dedicated
/// audio thread, and results come back as [`AudioEvent`]s.
pub struct AudioEngine {
    commands: mpsc::Sender<Command>,
    thread: Option<JoinHandle<()>>,
}

impl AudioEngine {
    /// Start the engine and open the default output device. Without a
    /// working output (no sound device yet, or the sound server still
    /// starting) the engine starts anyway and tries again at each song.
    pub fn new(on_event: impl Fn(AudioEvent) + Send + 'static) -> Result<Self, AudioError> {
        let (commands, receiver) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let loopback = commands.clone();

        // The output stream must stay on the thread that created it.
        let thread = thread::Builder::new()
            .name("veena-audio".into())
            .spawn(move || {
                let broken = Arc::new(AtomicBool::new(false));
                let output = match open_output(broken.clone()) {
                    Ok(mut output) => {
                        output.log_on_drop(false);
                        Some(output)
                    }
                    Err(e) => {
                        log::warn!(target: "veena_audio", "no audio output yet: {e}");
                        None
                    }
                };
                let _ = ready_tx.send(Ok::<(), AudioError>(()));
                Worker::new(output, broken, loopback, Box::new(on_event)).run(receiver);
            })?;

        ready_rx
            .recv()
            .map_err(|_| AudioError::Output("audio thread exited".into()))??;
        Ok(Self {
            commands,
            thread: Some(thread),
        })
    }

    /// Start loading a stream, replacing whatever is playing.
    /// Play `stream`. `cache`, if given, keeps a network stream for offline
    /// listening once it has downloaded in full.
    pub fn load(&self, stream: Stream, autoplay: bool, cache: Option<CacheWriter>) {
        self.send(Command::Load {
            stream,
            autoplay,
            cache: cache.map(Box::new),
        });
    }

    pub fn play(&self) {
        self.send(Command::Play);
    }

    pub fn pause(&self) {
        self.send(Command::Pause);
    }

    pub fn stop(&self) {
        self.send(Command::Stop);
    }

    pub fn seek(&self, position: Duration) {
        self.send(Command::Seek(position));
    }

    /// Volume from 0.0 (silent) to 1.0 (full).
    pub fn set_volume(&self, volume: f32) {
        self.send(Command::SetVolume(volume));
    }

    fn send(&self, command: Command) {
        // Only fails if the audio thread is gone, which is reported on start.
        let _ = self.commands.send(command);
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.send(Command::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Open the default audio output.
/// Open the audio output. `broken` is set if the system later stops the
/// stream, for example when headphones or a Bluetooth device connect or
/// disconnect on Android; the engine then opens a new one.
fn open_output(broken: Arc<AtomicBool>) -> Result<OutputStream, AudioError> {
    let on_error = move |e: rodio::cpal::StreamError| {
        log::warn!(target: "veena_audio", "audio output stopped: {e}");
        broken.store(true, Ordering::Relaxed);
    };
    // Some platform audio code panics instead of returning an error; report
    // that as an error too.
    std::panic::catch_unwind(move || open_platform_output(on_error)).unwrap_or_else(|panic| {
        let message = panic
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| panic.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown error".into());
        Err(AudioError::Output(format!(
            "audio system failed: {message}"
        )))
    })
}

#[cfg(not(target_os = "android"))]
fn open_platform_output(
    on_error: impl FnMut(rodio::cpal::StreamError) + Send + Clone + 'static,
) -> Result<OutputStream, AudioError> {
    OutputStreamBuilder::from_default_device()
        .and_then(|builder| {
            builder
                .with_error_callback(on_error)
                .open_stream_or_fallback()
        })
        .map_err(|e| AudioError::Output(e.to_string()))
}

/// On Android, asking the device for its preferred format goes through
/// Java, which needs an app context (missing in Termux, and only set up
/// later in the app). Playback itself does not, so open the output with a
/// standard format; AAudio converts it for the device.
#[cfg(target_os = "android")]
fn open_platform_output(
    on_error: impl FnMut(rodio::cpal::StreamError) + Send + Clone + 'static,
) -> Result<OutputStream, AudioError> {
    use rodio::cpal::traits::HostTrait;

    let device = rodio::cpal::default_host()
        .default_output_device()
        .ok_or_else(|| AudioError::Output("no audio output device".into()))?;
    OutputStreamBuilder::default()
        .with_device(device)
        .with_sample_rate(48_000)
        .with_channels(2)
        .with_error_callback(on_error)
        .open_stream()
        .map_err(|e| AudioError::Output(e.to_string()))
}

/// State owned by the audio thread.
struct Worker {
    /// None until an output could be opened.
    output: Option<OutputStream>,
    /// Set when the system stops the output stream.
    broken: Arc<AtomicBool>,
    /// What is loaded, to start it again on a new output.
    current: Option<Stream>,
    loopback: mpsc::Sender<Command>,
    on_event: Box<dyn Fn(AudioEvent) + Send>,
    sink: Option<Sink>,
    /// Bumped on every load or stop so stale background loads are ignored.
    generation: u64,
    state: PlaybackState,
    duration: Option<Duration>,
    volume: f32,
    last_position: Instant,
}

impl Worker {
    fn new(
        output: Option<OutputStream>,
        broken: Arc<AtomicBool>,
        loopback: mpsc::Sender<Command>,
        on_event: Box<dyn Fn(AudioEvent) + Send>,
    ) -> Self {
        Self {
            output,
            broken,
            current: None,
            loopback,
            on_event,
            sink: None,
            generation: 0,
            state: PlaybackState::Stopped,
            duration: None,
            volume: 1.0,
            last_position: Instant::now(),
        }
    }

    fn run(mut self, commands: mpsc::Receiver<Command>) {
        loop {
            match commands.recv_timeout(TICK) {
                Ok(Command::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
                Ok(command) => self.handle(command),
                Err(RecvTimeoutError::Timeout) => {}
            }
            self.tick();
        }
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
    }

    fn handle(&mut self, command: Command) {
        match command {
            Command::Load {
                stream,
                autoplay,
                cache,
            } => {
                self.clear();
                self.current = Some(stream.clone());
                self.set_state(PlaybackState::Loading);
                let generation = self.generation;
                let loopback = self.loopback.clone();
                thread::spawn(move || {
                    let result = decode::open(&stream, cache.map(|c| *c));
                    let _ = loopback.send(Command::Loaded {
                        generation,
                        autoplay,
                        result,
                    });
                });
            }
            Command::Loaded {
                generation,
                autoplay,
                result,
            } => {
                if generation != self.generation {
                    return; // superseded by a newer load or a stop
                }
                match result {
                    Ok(decoder) => {
                        self.duration = decoder.total_duration();
                        let Some(mixer) = self.mixer() else {
                            self.emit(AudioEvent::Error(
                                "no audio output: check that a speaker or headphones are available"
                                    .into(),
                            ));
                            self.set_state(PlaybackState::Stopped);
                            return;
                        };
                        let sink = Sink::connect_new(&mixer);
                        sink.set_volume(self.volume);
                        if !autoplay {
                            sink.pause();
                        }
                        sink.append(decoder);
                        self.sink = Some(sink);
                        self.set_state(if autoplay {
                            PlaybackState::Playing
                        } else {
                            PlaybackState::Paused
                        });
                        self.emit_position();
                    }
                    Err(e) => {
                        self.emit(AudioEvent::Error(e.to_string()));
                        self.set_state(PlaybackState::Stopped);
                    }
                }
            }
            Command::Reopened {
                generation,
                position,
                playing,
                result,
            } => {
                if generation != self.generation {
                    return;
                }
                match result {
                    Ok(decoder) => {
                        let Some(mixer) = self.mixer() else { return };
                        let sink = Sink::connect_new(&mixer);
                        sink.set_volume(self.volume);
                        sink.append(decoder);
                        if let Err(e) = sink.try_seek(position) {
                            log::warn!(target: "veena_audio", "could not return to {position:?}: {e}");
                        }
                        if !playing {
                            sink.pause();
                        }
                        self.sink = Some(sink);
                        log::info!(target: "veena_audio", "resumed on the new audio output");
                        self.emit_position();
                    }
                    Err(e) => {
                        self.emit(AudioEvent::Error(e.to_string()));
                        self.set_state(PlaybackState::Stopped);
                    }
                }
            }
            Command::Play => {
                if let Some(sink) = &self.sink {
                    sink.play();
                    self.set_state(PlaybackState::Playing);
                }
            }
            Command::Pause => {
                if let Some(sink) = &self.sink {
                    sink.pause();
                    self.set_state(PlaybackState::Paused);
                }
            }
            Command::Stop => {
                self.clear();
                self.set_state(PlaybackState::Stopped);
            }
            Command::Seek(position) => {
                if let Some(sink) = &self.sink {
                    if let Err(e) = sink.try_seek(position) {
                        self.emit(AudioEvent::Error(format!("seek failed: {e}")));
                    }
                    self.emit_position();
                }
            }
            Command::SetVolume(volume) => {
                self.volume = volume.clamp(0.0, 1.0);
                if let Some(sink) = &self.sink {
                    sink.set_volume(self.volume);
                }
            }
            Command::Shutdown => unreachable!("handled in run"),
        }
    }

    /// The output's mixer, opening an output first if there is none yet.
    fn mixer(&mut self) -> Option<rodio::mixer::Mixer> {
        if self.output.is_none() {
            match open_output(self.broken.clone()) {
                Ok(mut output) => {
                    output.log_on_drop(false);
                    log::info!(target: "veena_audio", "opened the audio output");
                    self.output = Some(output);
                }
                Err(e) => log::warn!(target: "veena_audio", "still no audio output: {e}"),
            }
        }
        self.output.as_ref().map(|o| o.mixer().clone())
    }

    /// The system stopped the output (a device was connected or removed):
    /// open a new one and carry on from the same place.
    fn reopen_output(&mut self) {
        let mut output = match open_output(self.broken.clone()) {
            Ok(output) => output,
            Err(e) => {
                log::warn!(target: "veena_audio", "could not open a new audio output: {e}");
                return;
            }
        };
        output.log_on_drop(false);
        log::info!(target: "veena_audio", "opened a new audio output");
        let old = self.sink.take();
        self.output = Some(output);
        let (Some(old), Some(stream)) = (old, self.current.clone()) else {
            return;
        };
        let position = old.get_pos();
        let playing = !old.is_paused();
        old.stop();
        let generation = self.generation;
        let loopback = self.loopback.clone();
        thread::spawn(move || {
            let result = decode::open(&stream, None);
            let _ = loopback.send(Command::Reopened {
                generation,
                position,
                playing,
                result,
            });
        });
    }

    fn tick(&mut self) {
        if self.broken.swap(false, Ordering::Relaxed) {
            self.reopen_output();
        }
        let Some(sink) = &self.sink else { return };
        if sink.empty() {
            self.sink = None;
            self.duration = None;
            self.set_state(PlaybackState::Stopped);
            self.emit(AudioEvent::TrackEnded);
        } else if self.state == PlaybackState::Playing
            && self.last_position.elapsed() >= POSITION_INTERVAL
        {
            self.emit_position();
        }
    }

    /// Drop the current track and cancel any pending load.
    fn clear(&mut self) {
        self.current = None;
        self.generation += 1;
        self.duration = None;
        if let Some(sink) = self.sink.take() {
            sink.stop();
        }
    }

    fn set_state(&mut self, state: PlaybackState) {
        if state != self.state {
            self.state = state;
            self.emit(AudioEvent::StateChanged(state));
        }
    }

    fn emit_position(&mut self) {
        if let Some(sink) = &self.sink {
            self.last_position = Instant::now();
            let event = AudioEvent::Position {
                position: sink.get_pos(),
                duration: self.duration,
            };
            self.emit(event);
        }
    }

    fn emit(&self, event: AudioEvent) {
        (self.on_event)(event);
    }
}
