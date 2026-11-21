use async_trait::async_trait;

use crate::{Error, Result, Stream, Track};

/// The search slot: finds music.
///
/// Catalogs return tracks labelled with a content kind (see
/// [`crate::TrackId::kind`]); any resolver that handles that kind can then
/// play them.
#[async_trait]
pub trait Catalog: Send + Sync {
    /// Stable identifier, e.g. `youtube-music`.
    fn id(&self) -> &str;

    /// Human-readable name, e.g. `YouTube Music`.
    fn name(&self) -> &str;

    /// Find tracks matching a free-text query.
    async fn search(&self, query: &str) -> Result<Vec<Track>>;

    /// List the tracks of a playlist or album.
    async fn playlist(&self, _id: &str) -> Result<Vec<Track>> {
        Err(Error::Unsupported {
            source_id: self.id().to_owned(),
            operation: "playlists",
        })
    }

    /// Songs that go well after `track`, best first (a "radio" for it).
    async fn related(&self, _track: &Track) -> Result<Vec<Track>> {
        Err(Error::Unsupported {
            source_id: self.id().to_owned(),
            operation: "related songs",
        })
    }
}

/// The audio-link slot: turns a track into playable streams.
#[async_trait]
pub trait Resolver: Send + Sync {
    /// Stable identifier, e.g. `youtube-audio`.
    fn id(&self) -> &str;

    /// Human-readable name.
    fn name(&self) -> &str;

    /// Content kinds this resolver can play, e.g. `["youtube"]`.
    fn kinds(&self) -> Vec<String>;

    /// Playable streams for a track, best first.
    async fn resolve(&self, track: &Track) -> Result<Vec<Stream>>;
}
