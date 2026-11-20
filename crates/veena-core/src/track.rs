use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Globally unique track identifier: which source it came from, plus the
/// source's own id for it (a file path, a video id, ...).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TrackId {
    pub source: String,
    pub id: String,
}

impl TrackId {
    pub fn new(source: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            id: id.into(),
        }
    }
}

impl fmt::Display for TrackId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.source, self.id)
    }
}

/// Metadata for one playable item.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<Duration>,
    /// URL or file path of cover art.
    pub artwork: Option<String>,
}

impl Track {
    pub fn new(id: TrackId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            artist: None,
            album: None,
            duration: None,
            artwork: None,
        }
    }

    pub fn artist_or_unknown(&self) -> &str {
        self.artist.as_deref().unwrap_or("Unknown artist")
    }
}

/// Where the audio bytes for a track can be read from.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamLocation {
    File(PathBuf),
    Http {
        url: String,
        /// Extra request headers some services require.
        headers: Vec<(String, String)>,
    },
}

/// A concrete, playable rendition of a track.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stream {
    pub location: StreamLocation,
    /// MIME type if known, e.g. `audio/mp4`. Helps the decoder pick a format.
    pub mime: Option<String>,
    /// Bits per second if known. Used to choose between renditions.
    pub bitrate: Option<u32>,
}

impl Stream {
    pub fn file(path: impl Into<PathBuf>) -> Self {
        Self {
            location: StreamLocation::File(path.into()),
            mime: None,
            bitrate: None,
        }
    }

    pub fn http(url: impl Into<String>) -> Self {
        Self {
            location: StreamLocation::Http {
                url: url.into(),
                headers: Vec::new(),
            },
            mime: None,
            bitrate: None,
        }
    }

    pub fn with_mime(mut self, mime: impl Into<String>) -> Self {
        self.mime = Some(mime.into());
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        if let StreamLocation::Http { headers, .. } = &mut self.location {
            headers.push((name.into(), value.into()));
        }
        self
    }
}
