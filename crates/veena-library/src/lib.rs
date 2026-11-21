//! The listener's own music: liked songs, recently played and playlists.
//!
//! Everything is kept in one small JSON file, written whole after each
//! change (to a temporary file first, so a crash never leaves it half
//! written). A damaged file is set aside and a fresh library started.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use veena_core::{Track, TrackId};

/// Most songs kept in "recently played".
pub const HISTORY_LIMIT: usize = 200;
const FORMAT: u32 = 1;

/// A track together with the name of the catalog it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub track: Track,
    #[serde(default)]
    pub source: String,
}

/// A song in the history, with when it was played (seconds since 1970).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Played {
    pub entry: Entry,
    pub at: u64,
}

/// A song the listener skipped early, and how often.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Skip {
    pub id: TrackId,
    #[serde(default)]
    pub artist: Option<String>,
    pub count: u32,
    pub at: u64,
}

/// Most skipped songs remembered.
const SKIP_LIMIT: usize = 500;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub tracks: Vec<Entry>,
    pub created: u64,
    pub updated: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Data {
    #[serde(default)]
    format: u32,
    /// Newest first.
    #[serde(default)]
    liked: Vec<Entry>,
    /// Newest first.
    #[serde(default)]
    history: Vec<Played>,
    #[serde(default)]
    playlists: Vec<Playlist>,
    /// Songs skipped in their first seconds, most recent first. Used to
    /// leave them out of recommendations.
    #[serde(default)]
    skips: Vec<Skip>,
}

pub struct Library {
    path: PathBuf,
    data: Mutex<Data>,
}

impl Library {
    /// Open the library stored at `path`, or start an empty one.
    pub fn open(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let data = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|e| {
                let aside = path.with_extension("damaged.json");
                log::warn!("library file unreadable ({e}); kept as {}", aside.display());
                let _ = fs::rename(&path, aside);
                Data::default()
            }),
            Err(_) => Data::default(),
        };
        Self {
            path,
            data: Mutex::new(data),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn change<T>(&self, f: impl FnOnce(&mut Data) -> T) -> T {
        let mut data = self.data.lock().unwrap_or_else(|e| e.into_inner());
        let result = f(&mut data);
        data.format = FORMAT;
        if let Err(e) = save(&self.path, &data) {
            log::warn!("could not save the library: {e}");
        }
        result
    }

    fn read<T>(&self, f: impl FnOnce(&Data) -> T) -> T {
        f(&self.data.lock().unwrap_or_else(|e| e.into_inner()))
    }

    // ------------------------------------------------------------ Liked

    /// Liked songs, most recently liked first.
    pub fn liked(&self) -> Vec<Entry> {
        self.read(|d| d.liked.clone())
    }

    pub fn is_liked(&self, id: &TrackId) -> bool {
        self.read(|d| d.liked.iter().any(|e| &e.track.id == id))
    }

    /// Like or unlike a song.
    pub fn set_liked(&self, entry: Entry, liked: bool) {
        self.change(|d| {
            d.liked.retain(|e| e.track.id != entry.track.id);
            if liked {
                d.liked.insert(0, entry);
            }
        });
    }

    // ------------------------------------------------------------ History

    /// Recently played songs, newest first, each song once.
    pub fn history(&self) -> Vec<Played> {
        self.read(|d| d.history.clone())
    }

    /// Note that a song started playing.
    pub fn record_played(&self, entry: Entry) {
        self.change(|d| {
            d.history.retain(|p| p.entry.track.id != entry.track.id);
            d.history.insert(0, Played { entry, at: now() });
            d.history.truncate(HISTORY_LIMIT);
        });
    }

    pub fn clear_history(&self) {
        self.change(|d| {
            d.history.clear();
            d.skips.clear();
        });
    }

    // ------------------------------------------------------------ Skips

    /// Note that a song was skipped soon after it started.
    pub fn record_skip(&self, track: &Track) {
        self.change(|d| {
            let count = d
                .skips
                .iter()
                .find(|s| s.id == track.id)
                .map_or(0, |s| s.count);
            d.skips.retain(|s| s.id != track.id);
            d.skips.insert(
                0,
                Skip {
                    id: track.id.clone(),
                    artist: track.artist.clone(),
                    count: count + 1,
                    at: now(),
                },
            );
            d.skips.truncate(SKIP_LIMIT);
        });
    }

    /// Skipped songs, most recently skipped first.
    pub fn skips(&self) -> Vec<Skip> {
        self.read(|d| d.skips.clone())
    }

    // ------------------------------------------------------------ Playlists

    /// All playlists, most recently changed first.
    pub fn playlists(&self) -> Vec<Playlist> {
        self.read(|d| {
            let mut lists = d.playlists.clone();
            lists.sort_by_key(|p| std::cmp::Reverse(p.updated));
            lists
        })
    }

    pub fn playlist(&self, id: &str) -> Option<Playlist> {
        self.read(|d| d.playlists.iter().find(|p| p.id == id).cloned())
    }

    /// Create an empty playlist and return its id.
    pub fn create_playlist(&self, name: &str) -> String {
        let id = new_id();
        let at = now();
        self.change(|d| {
            d.playlists.push(Playlist {
                id: id.clone(),
                name: clean_name(name),
                tracks: Vec::new(),
                created: at,
                updated: at,
            })
        });
        id
    }

    pub fn rename_playlist(&self, id: &str, name: &str) -> bool {
        self.edit_playlist(id, |p| p.name = clean_name(name))
    }

    pub fn delete_playlist(&self, id: &str) -> bool {
        self.change(|d| {
            let before = d.playlists.len();
            d.playlists.retain(|p| p.id != id);
            d.playlists.len() != before
        })
    }

    /// Add a song to the end of a playlist.
    pub fn add_to_playlist(&self, id: &str, entry: Entry) -> bool {
        self.edit_playlist(id, |p| p.tracks.push(entry))
    }

    pub fn remove_from_playlist(&self, id: &str, index: usize) -> bool {
        self.edit_playlist(id, |p| {
            if index < p.tracks.len() {
                p.tracks.remove(index);
            }
        })
    }

    pub fn move_in_playlist(&self, id: &str, from: usize, to: usize) -> bool {
        self.edit_playlist(id, |p| {
            if from < p.tracks.len() && to < p.tracks.len() {
                let entry = p.tracks.remove(from);
                p.tracks.insert(to, entry);
            }
        })
    }

    fn edit_playlist(&self, id: &str, f: impl FnOnce(&mut Playlist)) -> bool {
        self.change(|d| match d.playlists.iter_mut().find(|p| p.id == id) {
            Some(p) => {
                f(p);
                p.updated = now();
                true
            }
            None => false,
        })
    }
}

fn save(path: &Path, data: &Data) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec(data)?)?;
    fs::rename(tmp, path)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn new_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{nanos:x}{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
}

fn clean_name(name: &str) -> String {
    let name: String = name.trim().chars().take(100).collect();
    if name.is_empty() {
        "New playlist".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str) -> Entry {
        Entry {
            track: Track::new(TrackId::new("test", id), id),
            source: "Test".into(),
        }
    }

    fn temp_path(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("veena-library-{name}-{}", new_id()));
        dir.join("library.json")
    }

    #[test]
    fn keeps_everything_across_restarts() {
        let path = temp_path("restart");
        {
            let lib = Library::open(&path);
            lib.set_liked(entry("a"), true);
            lib.record_played(entry("b"));
            let id = lib.create_playlist("  Road trip  ");
            lib.add_to_playlist(&id, entry("c"));
            lib.add_to_playlist(&id, entry("d"));
            lib.move_in_playlist(&id, 1, 0);
        }
        let lib = Library::open(&path);
        assert!(lib.is_liked(&TrackId::new("test", "a")));
        assert_eq!(lib.history()[0].entry, entry("b"));
        let list = &lib.playlists()[0];
        assert_eq!(list.name, "Road trip");
        let titles: Vec<_> = list.tracks.iter().map(|e| e.track.title.as_str()).collect();
        assert_eq!(titles, ["d", "c"]);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn history_lists_each_song_once_newest_first() {
        let lib = Library::open(temp_path("history"));
        for id in ["a", "b", "a"] {
            lib.record_played(entry(id));
        }
        let ids: Vec<_> = lib
            .history()
            .iter()
            .map(|p| p.entry.track.id.id.clone())
            .collect();
        assert_eq!(ids, ["a", "b"]);
        for i in 0..HISTORY_LIMIT + 5 {
            lib.record_played(entry(&i.to_string()));
        }
        assert_eq!(lib.history().len(), HISTORY_LIMIT);
        fs::remove_dir_all(lib.path().parent().unwrap()).unwrap();
    }

    #[test]
    fn skips_are_counted() {
        let path = temp_path("skips");
        let library = Library::open(&path);
        let song = entry("a").track;
        library.record_skip(&song);
        library.record_skip(&song);
        library.record_skip(&entry("b").track);
        let skips = Library::open(&path).skips();
        assert_eq!(skips.len(), 2);
        assert_eq!(skips[1].id, song.id);
        assert_eq!(skips[1].count, 2);
        let _ = fs::remove_file(path);
    }

    #[test]
    fn unlike_and_delete() {
        let lib = Library::open(temp_path("delete"));
        lib.set_liked(entry("a"), true);
        lib.set_liked(entry("a"), true);
        assert_eq!(lib.liked().len(), 1);
        lib.set_liked(entry("a"), false);
        assert!(lib.liked().is_empty());
        let id = lib.create_playlist("");
        assert_eq!(lib.playlist(&id).unwrap().name, "New playlist");
        assert!(lib.delete_playlist(&id));
        assert!(!lib.delete_playlist(&id));
        fs::remove_dir_all(lib.path().parent().unwrap()).unwrap();
    }

    #[test]
    fn a_damaged_file_is_set_aside() {
        let path = temp_path("damaged");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{ not json").unwrap();
        let lib = Library::open(&path);
        assert!(lib.liked().is_empty());
        assert!(path.with_extension("damaged.json").exists());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
