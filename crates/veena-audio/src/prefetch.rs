//! Read-ahead for network streams.
//!
//! The decoder runs on the audio output's thread, which must never wait.
//! Reading straight from the network made it wait whenever a new range
//! request started or the connection slowed, and the sound skipped. Here a
//! download thread stays several megabytes ahead and the decoder only copies
//! from memory; it waits only if the network falls further behind than that.

use std::io::{self, Read, Seek, SeekFrom};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;
use std::time::Duration;

use crate::{CacheWriter, HttpReader};

/// How far ahead of the decoder the download runs.
const READ_AHEAD: u64 = 8 * 1024 * 1024;
/// Data kept behind the decoder for small backward seeks.
const KEEP_BEHIND: u64 = 2 * 1024 * 1024;
/// A seek this close past the downloaded data waits for it instead of
/// starting a new download.
const WAIT_AHEAD: u64 = 1024 * 1024;
/// Bytes read from the network at a time.
const READ_SIZE: usize = 64 * 1024;
/// Give up if no data arrives for this long.
const STALL_TIMEOUT: Duration = Duration::from_secs(30);

pub struct PrefetchReader {
    shared: Arc<Shared>,
    pos: u64,
    len: Option<u64>,
    seekable: bool,
    content_type: Option<String>,
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
}

struct State {
    /// File offset of `data[0]`.
    base: u64,
    data: Vec<u8>,
    /// The download reached the end of the file (or failed: see `error`).
    done: bool,
    error: Option<String>,
    /// The decoder wants the download to start again from here.
    restart: Option<u64>,
    /// Where the decoder is reading, to limit read-ahead and trim old data.
    reader_pos: u64,
    closed: bool,
}

impl State {
    fn end(&self) -> u64 {
        self.base + self.data.len() as u64
    }
}

impl PrefetchReader {
    /// Start downloading from `reader`'s current position (the start).
    /// `cache`, if given, keeps the song once it has downloaded in full.
    pub fn new(reader: HttpReader, cache: Option<CacheWriter>) -> Self {
        let len = reader.byte_len();
        let seekable = reader.is_seekable();
        let content_type = reader.content_type().map(str::to_owned);
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                base: 0,
                data: Vec::new(),
                done: false,
                error: None,
                restart: None,
                reader_pos: 0,
                closed: false,
            }),
            changed: Condvar::new(),
        });
        let worker = shared.clone();
        thread::Builder::new()
            .name("veena-download".into())
            .spawn(move || download(reader, &worker, cache))
            .expect("start download thread");
        Self {
            shared,
            pos: 0,
            len,
            seekable,
            content_type,
        }
    }

    pub fn byte_len(&self) -> Option<u64> {
        self.len
    }

    pub fn is_seekable(&self) -> bool {
        self.seekable
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The download thread: fill the buffer, restarting where the decoder asks.
fn download(mut reader: HttpReader, shared: &Shared, mut cache: Option<CacheWriter>) {
    let total = reader.byte_len();
    let mut chunk = vec![0u8; READ_SIZE];
    loop {
        let offset;
        {
            let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if state.closed {
                    return;
                }
                if let Some(pos) = state.restart.take() {
                    // A jump elsewhere: the song can't be kept whole this time.
                    if let Some(writer) = cache.as_mut() {
                        writer.abandon();
                    }
                    state.base = pos;
                    state.data.clear();
                    state.done = false;
                    state.error = None;
                    if let Err(e) = reader.seek(SeekFrom::Start(pos)) {
                        state.error = Some(e.to_string());
                        state.done = true;
                    }
                    shared.changed.notify_all();
                    continue;
                }
                let ahead = state.end().saturating_sub(state.reader_pos);
                if !state.done && ahead < READ_AHEAD {
                    break;
                }
                state = shared
                    .changed
                    .wait(state)
                    .unwrap_or_else(|e| e.into_inner());
            }
            offset = state.end();
        }

        // The network read and the cache writes happen without the lock, so
        // the decoder can keep copying what is already here.
        let result = reader.read(&mut chunk);

        let mut state = shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.restart.is_some() {
            // The decoder moved elsewhere while this was read: drop it.
            continue;
        }
        let mut finished = false;
        let mut failed = false;
        let mut received = 0;
        match result {
            Ok(0) => {
                state.done = true;
                finished = true;
            }
            Ok(n) => {
                received = n;
                state.data.extend_from_slice(&chunk[..n]);
                // The whole file is here: keep it now, without waiting for
                // the decoder (which may close the stream at the last byte).
                if total == Some(state.end()) {
                    state.done = true;
                    finished = true;
                }
                // Forget data well behind the decoder.
                let behind = state.reader_pos.saturating_sub(state.base);
                if behind > KEEP_BEHIND * 2 {
                    let drop = (behind - KEEP_BEHIND) as usize;
                    state.data.drain(..drop);
                    state.base += drop as u64;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => {
                state.error = Some(e.to_string());
                state.done = true;
                failed = true;
            }
        }
        shared.changed.notify_all();
        drop(state);

        if let Some(writer) = cache.as_mut() {
            if received > 0 {
                writer.write_at(offset, &chunk[..received]);
            }
            if failed {
                writer.abandon();
            }
        }
        if finished && let Some(writer) = cache.take() {
            writer.finish(total);
        }
    }
}

impl Read for PrefetchReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() || self.len.is_some_and(|len| self.pos >= len) {
            return Ok(0);
        }
        let pos = self.pos;
        let shared = self.shared.clone();
        let mut state = self.lock();
        state.reader_pos = pos;
        loop {
            if state.restart.is_none() && pos >= state.base && pos < state.end() {
                let from = (pos - state.base) as usize;
                let n = buf.len().min(state.data.len() - from);
                buf[..n].copy_from_slice(&state.data[from..from + n]);
                state.reader_pos = pos + n as u64;
                shared.changed.notify_all();
                drop(state);
                self.pos += n as u64;
                return Ok(n);
            }
            let coming =
                state.restart.is_none() && pos >= state.base && pos <= state.end() + WAIT_AHEAD;
            if coming && state.done {
                return match state.error.take() {
                    Some(e) => Err(io::Error::other(e)),
                    None => Ok(0),
                };
            }
            if !coming && state.restart != Some(pos) {
                state.restart = Some(pos);
                shared.changed.notify_all();
            }
            let (next, timeout) = shared
                .changed
                .wait_timeout(state, STALL_TIMEOUT)
                .unwrap_or_else(|e| e.into_inner());
            state = next;
            if timeout.timed_out() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "audio download stalled",
                ));
            }
        }
    }
}

impl Seek for PrefetchReader {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let target = match from {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::Current(delta) => self.pos.checked_add_signed(delta),
            SeekFrom::End(delta) => match self.len {
                Some(len) => len.checked_add_signed(delta),
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::Unsupported,
                        "stream length is unknown",
                    ));
                }
            },
        }
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "seek before start"))?;
        // The next read fetches from here, from memory if it is already in.
        self.pos = target;
        Ok(target)
    }
}

impl Drop for PrefetchReader {
    fn drop(&mut self) {
        self.lock().closed = true;
        self.shared.changed.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::tests::{data, serve};

    fn reader() -> (PrefetchReader, Vec<u8>) {
        let data = data();
        let http = HttpReader::open(&serve(data.clone(), true), &[]).unwrap();
        (PrefetchReader::new(http, None), data)
    }

    #[test]
    fn reads_the_whole_stream() {
        let (mut reader, data) = reader();
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        assert_eq!(out, data);
    }

    #[test]
    fn seeks_anywhere() {
        let (mut reader, data) = reader();
        let mut buf = [0u8; 16];
        for &pos in &[0u64, 10, 500_000, 400_000, 999_990, 3, 700_000, 699_000] {
            reader.seek(SeekFrom::Start(pos)).unwrap();
            let n = reader.read(&mut buf).unwrap();
            assert!(n > 0);
            assert_eq!(&buf[..n], &data[pos as usize..pos as usize + n], "at {pos}");
        }
        reader.seek(SeekFrom::End(0)).unwrap();
        assert_eq!(reader.read(&mut buf).unwrap(), 0);
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;
    use crate::http::tests::{data, serve};
    use crate::{AudioCache, CachedReader};
    use veena_core::{StreamLocation, Track, TrackId};

    #[test]
    fn a_song_played_through_is_kept_for_offline_listening() {
        let dir = std::env::temp_dir().join(format!("veena-prefetch-cache-{}", std::process::id()));
        let cache = AudioCache::open(&dir, 64 << 20);
        let track = Track::new(TrackId::new("test", "song"), "Song");
        let data = data();
        let http = HttpReader::open(&serve(data.clone(), true), &[]).unwrap();
        let mut reader = PrefetchReader::new(http, cache.writer(&track, None));
        let mut out = Vec::new();
        reader.read_to_end(&mut out).unwrap();
        drop(reader);
        // The download thread finishes the cache file just after the last read.
        for _ in 0..50 {
            if cache.contains(&track) {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let StreamLocation::Cached(path) = cache.lookup(&track).expect("kept").location else {
            panic!("not a cached stream");
        };
        let mut back = Vec::new();
        CachedReader::open(&path)
            .unwrap()
            .read_to_end(&mut back)
            .unwrap();
        assert_eq!(back, data);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
