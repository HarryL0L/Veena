//! Song lyrics, from a lyrics brick.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// One line of lyrics. `time` is when it is sung, for synced lyrics.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LyricLine {
    pub time: Option<Duration>,
    pub text: String,
}

/// A song's lyrics: timed lines that follow the music, or plain text.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Lyrics {
    pub lines: Vec<LyricLine>,
    /// The song has no words.
    pub instrumental: bool,
    /// Name of the brick that found them, for credit.
    pub source: String,
}

impl Lyrics {
    /// Whether every line has a time, so the lyrics can follow the song.
    pub fn synced(&self) -> bool {
        !self.lines.is_empty() && self.lines.iter().all(|l| l.time.is_some())
    }
}
