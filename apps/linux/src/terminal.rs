//! Interactive terminal front end: key controls and a live status line.

use std::io::{IsTerminal, Write};
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::terminal;
use tokio::sync::{broadcast, mpsc};
use veena_core::{Repeat, Track};
use veena_player::{PlaybackState, Player, PlayerEvent};

const SEEK_STEP_MS: i64 = 5_000;
const VOLUME_STEP: f32 = 0.05;
const BAR_WIDTH: usize = 24;

const HELP: &str = "space pause · ←/→ seek · n next · b back · ↑/↓ volume · s shuffle · r repeat · l queue · q quit";

pub struct Settings {
    pub volume: f32,
    pub shuffle: bool,
    pub repeat: Repeat,
}

/// Run until the queue ends or the user quits.
pub async fn run(player: &Player, settings: Settings) -> Result<()> {
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let mut events = player.subscribe();
    let mut keys = if interactive {
        Some(spawn_key_reader())
    } else {
        None
    };
    let _raw = if interactive {
        Some(RawMode::enable()?)
    } else {
        None
    };

    let mut screen = Screen {
        interactive,
        settings,
        state: PlaybackState::Stopped,
        track: None,
        position: Duration::ZERO,
        duration: None,
    };
    if interactive {
        screen.line(HELP);
    }

    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(PlayerEvent::QueueEnded) => break,
                Ok(event) => screen.on_event(event),
                Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => break,
            },
            Some(key) = recv_key(&mut keys) => {
                if !screen.on_key(player, key).await {
                    break;
                }
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    screen.finish();
    Ok(())
}

async fn recv_key(keys: &mut Option<mpsc::UnboundedReceiver<KeyEvent>>) -> Option<KeyEvent> {
    match keys {
        Some(keys) => keys.recv().await,
        None => std::future::pending().await,
    }
}

/// Read key presses on a blocking thread.
fn spawn_key_reader() -> mpsc::UnboundedReceiver<KeyEvent> {
    let (tx, rx) = mpsc::unbounded_channel();
    std::thread::spawn(move || {
        while let Ok(event) = event::read() {
            if let Event::Key(key) = event
                && key.kind != KeyEventKind::Release
                && tx.send(key).is_err()
            {
                break;
            }
        }
    });
    rx
}

/// Puts the terminal in raw mode and restores it when dropped.
struct RawMode;

impl RawMode {
    fn enable() -> Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
}

struct Screen {
    interactive: bool,
    settings: Settings,
    state: PlaybackState,
    track: Option<(usize, Track)>,
    position: Duration,
    duration: Option<Duration>,
}

impl Screen {
    fn on_event(&mut self, event: PlayerEvent) {
        match event {
            PlayerEvent::TrackChanged { index, track } => {
                let album = track
                    .album
                    .as_ref()
                    .map(|a| format!("  ·  {a}"))
                    .unwrap_or_default();
                self.line(&format!(
                    "{:>3}.  {}  ·  {}{album}",
                    index + 1,
                    track.title,
                    track.artist_or_unknown()
                ));
                self.position = Duration::ZERO;
                self.duration = track.duration;
                self.track = Some((index, track));
            }
            PlayerEvent::StateChanged(state) => self.state = state,
            PlayerEvent::Position { position, duration } => {
                self.position = position;
                self.duration = duration;
            }
            PlayerEvent::Error(message) => self.line(&format!("  !   {message}")),
            PlayerEvent::QueueChanged | PlayerEvent::QueueEnded => {}
        }
        self.status();
    }

    /// Handle a key press. Returns false to quit.
    async fn on_key(&mut self, player: &Player, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Char('c') if ctrl => return false,
            KeyCode::Char('q') | KeyCode::Esc => return false,
            KeyCode::Char(' ') | KeyCode::Char('p') => player.toggle_pause(),
            KeyCode::Char('n') => player.next(),
            KeyCode::Char('b') => player.previous(),
            KeyCode::Right => player.seek_by(SEEK_STEP_MS),
            KeyCode::Left => player.seek_by(-SEEK_STEP_MS),
            KeyCode::Up | KeyCode::Char('+') | KeyCode::Char('=') => {
                self.change_volume(player, VOLUME_STEP)
            }
            KeyCode::Down | KeyCode::Char('-') => self.change_volume(player, -VOLUME_STEP),
            KeyCode::Char('s') => {
                self.settings.shuffle = !self.settings.shuffle;
                player.set_shuffle(self.settings.shuffle);
            }
            KeyCode::Char('r') => {
                self.settings.repeat = match self.settings.repeat {
                    Repeat::Off => Repeat::All,
                    Repeat::All => Repeat::One,
                    Repeat::One => Repeat::Off,
                };
                player.set_repeat(self.settings.repeat);
            }
            KeyCode::Char('l') => {
                let snapshot = player.snapshot().await;
                for (i, track) in snapshot.tracks.iter().enumerate() {
                    let marker = if Some(i) == snapshot.current {
                        "▸"
                    } else {
                        " "
                    };
                    self.line(&format!(
                        "{marker}{:>3}.  {}  ·  {}",
                        i + 1,
                        track.title,
                        track.artist_or_unknown()
                    ));
                }
            }
            KeyCode::Char('h') | KeyCode::Char('?') => self.line(HELP),
            _ => {}
        }
        self.status();
        true
    }

    fn change_volume(&mut self, player: &Player, delta: f32) {
        self.settings.volume = ((self.settings.volume + delta) * 100.0)
            .round()
            .clamp(0.0, 100.0)
            / 100.0;
        player.set_volume(self.settings.volume);
    }

    /// Print a permanent line above the status line.
    fn line(&self, text: &str) {
        let mut out = std::io::stdout().lock();
        if self.interactive {
            let _ = write!(out, "\r\x1b[2K{text}\r\n");
        } else {
            let _ = writeln!(out, "{text}");
        }
        let _ = out.flush();
    }

    /// Redraw the status line in place.
    fn status(&self) {
        if !self.interactive {
            return;
        }
        let icon = match self.state {
            PlaybackState::Playing => "▶",
            PlaybackState::Paused => "⏸",
            PlaybackState::Loading => "…",
            PlaybackState::Stopped => "■",
        };
        let progress = match self.duration {
            Some(total) if !total.is_zero() => {
                let filled = ((self.position.as_secs_f64() / total.as_secs_f64())
                    * BAR_WIDTH as f64)
                    .round()
                    .clamp(0.0, BAR_WIDTH as f64) as usize;
                format!(
                    "{} / {}  {}{}",
                    format_time(self.position),
                    format_time(total),
                    "━".repeat(filled),
                    "─".repeat(BAR_WIDTH - filled)
                )
            }
            _ => format_time(self.position),
        };
        let mut flags = format!("vol {:.0}%", self.settings.volume * 100.0);
        if self.settings.shuffle {
            flags.push_str("  shuffle");
        }
        match self.settings.repeat {
            Repeat::Off => {}
            Repeat::All => flags.push_str("  repeat all"),
            Repeat::One => flags.push_str("  repeat one"),
        }
        let mut out = std::io::stdout().lock();
        let _ = write!(out, "\r\x1b[2K {icon}  {progress}   {flags}");
        let _ = out.flush();
    }

    fn finish(&self) {
        if self.interactive {
            let mut out = std::io::stdout().lock();
            let _ = write!(out, "\r\x1b[2K");
            let _ = out.flush();
        }
    }
}

/// Format a duration as `m:ss`, or `h:mm:ss` from one hour.
pub fn format_time(duration: Duration) -> String {
    let secs = duration.as_secs();
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_time;
    use std::time::Duration;

    #[test]
    fn formats_time() {
        assert_eq!(format_time(Duration::from_secs(5)), "0:05");
        assert_eq!(format_time(Duration::from_secs(225)), "3:45");
        assert_eq!(format_time(Duration::from_secs(3725)), "1:02:05");
    }
}
