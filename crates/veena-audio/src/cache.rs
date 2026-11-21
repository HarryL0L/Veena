//! The offline cache: songs kept while they stream, so they play again
//! without the internet.
//!
//! This is a cache, not a download feature. A song is only kept after it
//! has streamed in full, in the app's own storage, with its bytes scrambled
//! so the files are not usable as music files outside Veena. The oldest
//! songs are dropped when the cache grows past its size limit, and the
//! whole cache can be cleared at any time (Android may also clear it).

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use veena_core::{Stream, StreamLocation, Track};

const INDEX: &str = "index.json";
const EXTENSION: &str = "vc";

/// One cached song.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheEntry {
    pub track: Track,
    pub mime: Option<String>,
    pub size: u64,
    /// When it was last played (a clock that only moves forward).
    pub last_used: u64,
    file: String,
}

pub struct AudioCache {
    dir: PathBuf,
    entries: Mutex<HashMap<String, CacheEntry>>,
    limit: AtomicU64,
    enabled: AtomicBool,
    offline: AtomicBool,
}

impl AudioCache {
    /// Open (or create) the cache in `dir`, keeping at most `limit` bytes.
    pub fn open(dir: impl Into<PathBuf>, limit: u64) -> Arc<Self> {
        let dir = dir.into();
        let _ = fs::create_dir_all(&dir);
        let mut entries: HashMap<String, CacheEntry> = fs::read(dir.join(INDEX))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        entries.retain(|_, e| dir.join(&e.file).is_file());
        // Remove leftovers: unfinished songs and files the index lost.
        let known: Vec<String> = entries.values().map(|e| e.file.clone()).collect();
        if let Ok(files) = fs::read_dir(&dir) {
            for file in files.flatten() {
                let name = file.file_name().to_string_lossy().into_owned();
                if name != INDEX && !known.contains(&name) {
                    let _ = fs::remove_file(file.path());
                }
            }
        }
        let cache = Arc::new(Self {
            dir,
            entries: Mutex::new(entries),
            limit: AtomicU64::new(limit),
            enabled: AtomicBool::new(true),
            offline: AtomicBool::new(false),
        });
        cache.evict();
        cache
    }

    /// Whether songs are kept as they stream.
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Offline mode: only play songs that are in the cache.
    pub fn offline(&self) -> bool {
        self.offline.load(Ordering::Relaxed)
    }

    pub fn set_offline(&self, offline: bool) {
        self.offline.store(offline, Ordering::Relaxed);
    }

    pub fn limit(&self) -> u64 {
        self.limit.load(Ordering::Relaxed)
    }

    /// Change the size limit, dropping the oldest songs if needed.
    pub fn set_limit(&self, bytes: u64) {
        self.limit.store(bytes, Ordering::Relaxed);
        self.evict();
    }

    /// Bytes used by cached songs.
    pub fn usage(&self) -> u64 {
        self.lock().values().map(|e| e.size).sum()
    }

    /// Cached songs, most recently played first.
    pub fn entries(&self) -> Vec<CacheEntry> {
        let mut list: Vec<_> = self.lock().values().cloned().collect();
        list.sort_by_key(|e| std::cmp::Reverse(e.last_used));
        list
    }

    pub fn contains(&self, track: &Track) -> bool {
        self.lock().contains_key(&key(track))
    }

    /// A stream that plays the song from the cache, if it is there.
    pub fn lookup(&self, track: &Track) -> Option<Stream> {
        let mut entries = self.lock();
        let entry = entries.get_mut(&key(track))?;
        entry.last_used = now();
        let stream = Stream {
            location: StreamLocation::Cached(self.dir.join(&entry.file)),
            mime: entry.mime.clone(),
            bitrate: None,
        };
        self.save(&entries);
        Some(stream)
    }

    pub fn remove(&self, track: &Track) {
        let mut entries = self.lock();
        if let Some(entry) = entries.remove(&key(track)) {
            let _ = fs::remove_file(self.dir.join(entry.file));
            self.save(&entries);
        }
    }

    /// Remove every cached song.
    pub fn clear(&self) {
        let mut entries = self.lock();
        for entry in entries.values() {
            let _ = fs::remove_file(self.dir.join(&entry.file));
        }
        entries.clear();
        self.save(&entries);
    }

    /// A writer that keeps `track` once it has streamed in full, or `None`
    /// when caching is off or the song is already cached.
    pub fn writer(self: &Arc<Self>, track: &Track, mime: Option<String>) -> Option<CacheWriter> {
        if !self.enabled() || self.limit() == 0 || self.contains(track) {
            return None;
        }
        let seed = seed(&key(track));
        let tmp = self.dir.join(format!("{seed:016x}.part"));
        let file = File::create(&tmp).ok()?;
        Some(CacheWriter {
            cache: self.clone(),
            track: track.clone(),
            mime,
            file: Some(BufWriter::new(file)),
            tmp,
            seed,
            written: 0,
            broken: false,
        })
    }

    fn insert(&self, track: Track, mime: Option<String>, tmp: &Path, seed: u64, size: u64) {
        let file = format!("{seed:016x}.{EXTENSION}");
        if fs::rename(tmp, self.dir.join(&file)).is_err() {
            let _ = fs::remove_file(tmp);
            return;
        }
        let entry = CacheEntry {
            track: track.clone(),
            mime,
            size,
            last_used: now(),
            file,
        };
        {
            let mut entries = self.lock();
            entries.insert(key(&track), entry);
            self.save(&entries);
        }
        log::info!(
            "kept {} for offline listening ({} KB)",
            track.title,
            size / 1024
        );
        self.evict();
    }

    /// Drop the least recently played songs until the cache fits its limit.
    fn evict(&self) {
        let limit = self.limit();
        let mut entries = self.lock();
        let mut total: u64 = entries.values().map(|e| e.size).sum();
        if total <= limit {
            return;
        }
        let mut oldest: Vec<(String, u64, u64)> = entries
            .iter()
            .map(|(k, e)| (k.clone(), e.last_used, e.size))
            .collect();
        oldest.sort_by_key(|(_, used, _)| *used);
        for (k, _, size) in oldest {
            if total <= limit {
                break;
            }
            if let Some(entry) = entries.remove(&k) {
                let _ = fs::remove_file(self.dir.join(entry.file));
                total -= size;
            }
        }
        self.save(&entries);
    }

    fn save(&self, entries: &HashMap<String, CacheEntry>) {
        let tmp = self.dir.join("index.json.tmp");
        let ok = serde_json::to_vec(entries)
            .ok()
            .is_some_and(|bytes| fs::write(&tmp, bytes).is_ok());
        if ok {
            let _ = fs::rename(tmp, self.dir.join(INDEX));
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, CacheEntry>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Collects a song's bytes as they download; keeps them only if the whole
/// song arrives in order.
pub struct CacheWriter {
    cache: Arc<AudioCache>,
    track: Track,
    mime: Option<String>,
    file: Option<BufWriter<File>>,
    tmp: PathBuf,
    seed: u64,
    written: u64,
    broken: bool,
}

impl CacheWriter {
    /// Bytes downloaded at `offset`. Anything out of order (after a seek)
    /// means the song can't be kept whole this time.
    pub fn write_at(&mut self, offset: u64, data: &[u8]) {
        if self.broken {
            return;
        }
        if offset != self.written {
            self.abandon();
            return;
        }
        let mut scrambled = data.to_vec();
        scramble(self.seed, offset, &mut scrambled);
        match self.file.as_mut().map(|f| f.write_all(&scrambled)) {
            Some(Ok(())) => self.written += data.len() as u64,
            _ => self.abandon(),
        }
    }

    /// Stop collecting this song.
    pub fn abandon(&mut self) {
        self.broken = true;
        self.file = None;
        let _ = fs::remove_file(&self.tmp);
    }

    /// The download finished. Keep the song if it arrived whole.
    pub fn finish(mut self, total: Option<u64>) {
        let whole = !self.broken && self.written > 0 && total.is_none_or(|t| t == self.written);
        let flushed = self
            .file
            .take()
            .is_some_and(|mut f| f.flush().is_ok() && f.get_ref().sync_all().is_ok());
        if whole && flushed {
            self.broken = true; // nothing left to clean up in drop
            self.cache.insert(
                self.track.clone(),
                self.mime.take(),
                &self.tmp,
                self.seed,
                self.written,
            );
        } else {
            self.abandon();
        }
    }
}

impl Drop for CacheWriter {
    fn drop(&mut self) {
        if !self.broken {
            self.abandon();
        }
    }
}

/// Reads a cached song, unscrambling it.
pub struct CachedReader {
    file: File,
    seed: u64,
    pos: u64,
    len: u64,
}

impl CachedReader {
    pub fn open(path: &Path) -> io::Result<Self> {
        let seed = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| u64::from_str_radix(s, 16).ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a cached song"))?;
        let file = File::open(path)?;
        let len = file.metadata()?.len();
        Ok(Self {
            file,
            seed,
            pos: 0,
            len,
        })
    }

    pub fn len(&self) -> u64 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Read for CachedReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.file.read(buf)?;
        scramble(self.seed, self.pos, &mut buf[..n]);
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for CachedReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        self.pos = self.file.seek(from)?;
        Ok(self.pos)
    }
}

fn key(track: &Track) -> String {
    format!("{}:{}", track.id.kind, track.id.id)
}

/// A number derived from the song's key (FNV-1a), naming its file and
/// seeding the scrambling.
fn seed(key: &str) -> u64 {
    key.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0100_0000_01b3)
    })
}

/// XOR with a byte pattern drawn from `seed` (undone by doing it again).
fn scramble(seed: u64, offset: u64, data: &mut [u8]) {
    let mut pattern = [0u8; 64];
    let mut x = seed | 1;
    for chunk in pattern.chunks_mut(8) {
        // xorshift64
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        chunk.copy_from_slice(&x.to_le_bytes());
    }
    for (i, byte) in data.iter_mut().enumerate() {
        *byte ^= pattern[((offset + i as u64) % 64) as usize];
    }
}

/// Milliseconds since 1970, but always later than the last call, so the
/// order songs were played in is never ambiguous.
fn now() -> u64 {
    static LAST: AtomicU64 = AtomicU64::new(0);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let mut prev = LAST.load(Ordering::Relaxed);
    loop {
        let next = ms.max(prev + 1);
        match LAST.compare_exchange(prev, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(actual) => prev = actual,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use veena_core::TrackId;

    fn track(id: &str) -> Track {
        Track::new(TrackId::new("test", id), id)
    }

    fn temp_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("veena-cache-{name}-{}", std::process::id()))
    }

    fn keep(cache: &Arc<AudioCache>, id: &str, data: &[u8]) {
        let mut writer = cache.writer(&track(id), Some("audio/mp4".into())).unwrap();
        for (i, part) in data.chunks(1000).enumerate() {
            writer.write_at(i as u64 * 1000, part);
        }
        writer.finish(Some(data.len() as u64));
    }

    #[test]
    fn keeps_whole_songs_scrambled_and_reads_them_back() {
        let dir = temp_dir("whole");
        let cache = AudioCache::open(&dir, 1 << 20);
        let data: Vec<u8> = (0..5000u32).map(|i| (i % 251) as u8).collect();
        keep(&cache, "a", &data);

        let stream = cache.lookup(&track("a")).unwrap();
        let StreamLocation::Cached(path) = stream.location else {
            panic!()
        };
        // The file on disk is not the song.
        assert_ne!(fs::read(&path).unwrap(), data);
        let mut reader = CachedReader::open(&path).unwrap();
        let mut back = Vec::new();
        reader.read_to_end(&mut back).unwrap();
        assert_eq!(back, data);
        reader.seek(SeekFrom::Start(4321)).unwrap();
        let mut byte = [0u8];
        reader.read_exact(&mut byte).unwrap();
        assert_eq!(byte[0], data[4321]);

        // Survives a restart.
        drop(cache);
        let cache = AudioCache::open(&dir, 1 << 20);
        assert!(cache.contains(&track("a")));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn drops_songs_that_did_not_arrive_whole() {
        let dir = temp_dir("partial");
        let cache = AudioCache::open(&dir, 1 << 20);
        let mut writer = cache.writer(&track("a"), None).unwrap();
        writer.write_at(0, &[1; 100]);
        writer.write_at(500, &[2; 100]); // skipped ahead
        writer.finish(Some(600));
        let mut writer = cache.writer(&track("b"), None).unwrap();
        writer.write_at(0, &[1; 100]);
        writer.finish(Some(600)); // ended early
        drop(cache.writer(&track("c"), None)); // never finished
        assert_eq!(cache.entries().len(), 0);
        assert_eq!(
            fs::read_dir(&dir)
                .unwrap()
                .filter(|f| f.as_ref().unwrap().file_name() != INDEX)
                .count(),
            0
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn drops_the_oldest_songs_past_the_limit() {
        let dir = temp_dir("limit");
        let cache = AudioCache::open(&dir, 2500);
        keep(&cache, "a", &[0; 1000]);
        keep(&cache, "b", &[0; 1000]);
        keep(&cache, "c", &[0; 1000]);
        assert!(!cache.contains(&track("a")));
        assert!(cache.contains(&track("c")));
        assert!(cache.usage() <= 2500);
        cache.clear();
        assert_eq!(cache.usage(), 0);
        fs::remove_dir_all(dir).unwrap();
    }
}
