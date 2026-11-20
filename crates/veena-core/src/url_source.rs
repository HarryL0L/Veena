use async_trait::async_trait;

use crate::{Error, Resolver, Result, Stream, Track, TrackId};

/// Plays direct `http(s)://` links to audio files.
#[derive(Debug, Default)]
pub struct UrlSource;

impl UrlSource {
    pub const ID: &'static str = "url";

    /// Build a track for a URL. The title is the last path segment.
    pub fn track(url: &str) -> Track {
        let title = url
            .split(['?', '#'])
            .next()
            .and_then(|path| path.trim_end_matches('/').rsplit('/').next())
            .filter(|segment| !segment.is_empty())
            .unwrap_or(url);
        Track::new(TrackId::new(Self::ID, url), title)
    }

    pub fn is_url(input: &str) -> bool {
        input.starts_with("http://") || input.starts_with("https://")
    }
}

#[async_trait]
impl Resolver for UrlSource {
    fn id(&self) -> &str {
        Self::ID
    }

    fn name(&self) -> &str {
        "Direct links"
    }

    fn kinds(&self) -> Vec<String> {
        vec![Self::ID.to_owned()]
    }

    async fn resolve(&self, track: &Track) -> Result<Vec<Stream>> {
        if !Self::is_url(&track.id.id) {
            return Err(Error::NoStream(track.id.to_string()));
        }
        Ok(vec![Stream::http(&track.id.id)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_last_path_segment() {
        assert_eq!(
            UrlSource::track("https://x.org/a/song.mp3?t=1").title,
            "song.mp3"
        );
        assert_eq!(UrlSource::track("https://x.org/").title, "x.org");
    }
}
