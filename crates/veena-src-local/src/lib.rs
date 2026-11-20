//! Local music files as a Veena source.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use lofty::prelude::*;
use veena_core::{Catalog, Error, Resolver, Result, Stream, Track, TrackId, async_trait};
use walkdir::WalkDir;

/// File extensions Veena can decode.
pub const EXTENSIONS: &[&str] = &[
    "aac", "aif", "aiff", "flac", "m4a", "m4b", "mka", "mp3", "mp4", "oga", "ogg", "wav",
];

/// Music files from folders on this device.
#[derive(Debug, Default)]
pub struct LocalSource {
    roots: Vec<PathBuf>,
    tracks: RwLock<Vec<Track>>,
}

impl LocalSource {
    /// Brick id.
    pub const ID: &'static str = "local";
    /// Content kind of local files; track ids are file paths.
    pub const KIND: &'static str = "file";

    /// Create a source over the given folders and scan them.
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        let source = Self {
            roots: roots.into_iter().collect(),
            tracks: RwLock::default(),
        };
        source.rescan();
        source
    }

    /// Re-read all folders. Returns the number of tracks found.
    pub fn rescan(&self) -> usize {
        let tracks = scan(&self.roots);
        let count = tracks.len();
        *self.tracks.write().unwrap() = tracks;
        count
    }

    /// All tracks found by the last scan.
    pub fn tracks(&self) -> Vec<Track> {
        self.tracks.read().unwrap().clone()
    }

    /// Read one file into a track, using its tags when present.
    pub fn track_from_path(path: &Path) -> Track {
        let id = TrackId::new(Self::KIND, path.to_string_lossy());
        let fallback_title = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned());
        let mut track = Track::new(id, fallback_title);

        let Ok(file) = lofty::read_from_path(path) else {
            return track;
        };
        let duration = file.properties().duration();
        if !duration.is_zero() {
            track.duration = Some(duration);
        }
        if let Some(tag) = file.primary_tag().or_else(|| file.first_tag()) {
            let clean = |s: Option<std::borrow::Cow<'_, str>>| {
                s.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
            };
            if let Some(title) = clean(tag.title()) {
                track.title = title;
            }
            track.artist = clean(tag.artist());
            track.album = clean(tag.album());
        }
        track
    }

    /// Whether a path has a supported audio extension.
    pub fn is_audio(path: &Path) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
    }
}

/// Find and read every audio file under `roots`, sorted by path.
/// A root may also be a single file.
pub fn scan(roots: &[PathBuf]) -> Vec<Track> {
    let mut paths: Vec<PathBuf> = roots
        .iter()
        .flat_map(|root| WalkDir::new(root).follow_links(true))
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_file() && LocalSource::is_audio(entry.path()))
        .map(|entry| entry.into_path())
        .collect();
    paths.sort();
    paths.dedup();
    paths
        .iter()
        .map(|p| LocalSource::track_from_path(p))
        .collect()
}

#[async_trait]
impl Catalog for LocalSource {
    fn id(&self) -> &str {
        Self::ID
    }

    fn name(&self) -> &str {
        "Local files"
    }

    /// Case-insensitive match on title, artist, album or file path.
    /// Every word of the query must match somewhere.
    async fn search(&self, query: &str) -> Result<Vec<Track>> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let tracks = self.tracks.read().unwrap();
        Ok(tracks
            .iter()
            .filter(|track| {
                let haystack = [
                    Some(track.title.as_str()),
                    track.artist.as_deref(),
                    track.album.as_deref(),
                    Some(track.id.id.as_str()),
                ]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join("\n")
                .to_lowercase();
                words.iter().all(|word| haystack.contains(word))
            })
            .cloned()
            .collect())
    }
}

#[async_trait]
impl Resolver for LocalSource {
    fn id(&self) -> &str {
        Self::ID
    }

    fn name(&self) -> &str {
        "Local files"
    }

    fn kinds(&self) -> Vec<String> {
        vec![Self::KIND.to_owned()]
    }

    async fn resolve(&self, track: &Track) -> Result<Vec<Stream>> {
        let path = PathBuf::from(&track.id.id);
        if !path.is_file() {
            return Err(Error::NoStream(track.id.to_string()));
        }
        Ok(vec![Stream::file(path)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_and_searches_by_file_name() {
        let dir = std::env::temp_dir().join(format!("veena-local-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("Morning Raga.mp3"), b"").unwrap();
        std::fs::write(dir.join("sub/Evening.FLAC"), b"").unwrap();
        std::fs::write(dir.join("notes.txt"), b"").unwrap();

        let source = LocalSource::new([dir.clone()]);
        let titles: Vec<String> = source.tracks().into_iter().map(|t| t.title).collect();
        assert_eq!(titles, ["Morning Raga", "Evening"]);

        let found = block_on(source.search("raga morn"));
        assert_eq!(found.unwrap().len(), 1);
        let all = block_on(source.search(""));
        assert_eq!(all.unwrap().len(), 2);

        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Minimal executor: these futures never actually wait.
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        let mut future = std::pin::pin!(future);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
                return output;
            }
        }
    }
}
