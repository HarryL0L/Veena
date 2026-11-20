use async_trait::async_trait;

use crate::{Error, Result, Stream, Track};

/// A provider of music: local files, a streaming service, a plugin.
///
/// The player only talks to sources through this trait, so built-in sources
/// and (later) scripted plugins are interchangeable.
#[async_trait]
pub trait Source: Send + Sync {
    /// Stable identifier, used in [`crate::TrackId::source`]. Example: `local`.
    fn id(&self) -> &str;

    /// Human-readable name. Example: `Local files`.
    fn name(&self) -> &str;

    /// Find tracks matching a free-text query.
    async fn search(&self, query: &str) -> Result<Vec<Track>>;

    /// Turn a track into one or more playable streams, best first.
    async fn resolve(&self, track: &Track) -> Result<Vec<Stream>>;

    /// List the tracks of a playlist or album.
    async fn playlist(&self, _id: &str) -> Result<Vec<Track>> {
        Err(Error::Unsupported {
            source_id: self.id().to_owned(),
            operation: "playlists",
        })
    }
}
