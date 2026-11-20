use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use rodio::{Decoder, OutputStream, OutputStreamBuilder, Sink, Source};
use veena_core::Stream;

use crate::AudioError;
use crate::decode::{self, Input};

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
    },
    Loaded {
        generation: u64,
        autoplay: bool,
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
    /// Open the default output device and start the engine.
    pub fn new(on_event: impl Fn(AudioEvent) + Send + 'static) -> Result<Self, AudioError> {
        let (commands, receiver) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel();
        let loopback = commands.clone();

        // The output stream must stay on the thread that created it.
        let thread = thread::Builder::new()
            .name("veena-audio".into())
            .spawn(move || {
                let output = match open_output() {
                    Ok(mut output) => {
                        output.log_on_drop(false);
                        let _ = ready_tx.send(Ok(()));
                        output
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                Worker::new(output, loopback, Box::new(on_event)).run(receiver);
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
    pub fn load(&self, stream: Stream, autoplay: bool) {
        self.send(Command::Load { stream, autoplay });
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
fn open_output() -> Result<OutputStream, AudioError> {
    // Some platform audio code panics instead of returning an error; report
    // that as an error too.
    std::panic::catch_unwind(open_platform_output).unwrap_or_else(|panic| {
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
fn open_platform_output() -> Result<OutputStream, AudioError> {
    OutputStreamBuilder::open_default_stream().map_err(|e| AudioError::Output(e.to_string()))
}

/// On Android, asking the device for its preferred format goes through
/// Java, which needs an app context (missing in Termux, and only set up
/// later in the app). Playback itself does not, so open the output with a
/// standard format; AAudio converts it for the device.
#[cfg(target_os = "android")]
fn open_platform_output() -> Result<OutputStream, AudioError> {
    use rodio::cpal::traits::HostTrait;

    let device = rodio::cpal::default_host()
        .default_output_device()
        .ok_or_else(|| AudioError::Output("no audio output device".into()))?;
    OutputStreamBuilder::default()
        .with_device(device)
        .with_sample_rate(48_000)
        .with_channels(2)
        .open_stream()
        .map_err(|e| AudioError::Output(e.to_string()))
}

/// State owned by the audio thread.
struct Worker {
    output: OutputStream,
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
        output: OutputStream,
        loopback: mpsc::Sender<Command>,
        on_event: Box<dyn Fn(AudioEvent) + Send>,
    ) -> Self {
        Self {
            output,
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
            Command::Load { stream, autoplay } => {
                self.clear();
                self.set_state(PlaybackState::Loading);
                let generation = self.generation;
                let loopback = self.loopback.clone();
                thread::spawn(move || {
                    let result = decode::open(&stream);
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
                        let sink = Sink::connect_new(self.output.mixer());
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

    fn tick(&mut self) {
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
