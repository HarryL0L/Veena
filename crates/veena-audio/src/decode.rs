use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use rodio::Decoder;
use veena_core::{Stream, StreamLocation};

use crate::{AudioError, CacheWriter, CachedReader, HttpReader, PrefetchReader};

/// Byte source for the decoder: a local file or an HTTP stream.
pub(crate) enum Input {
    File(File),
    Cached(CachedReader),
    Http(PrefetchReader),
}

impl Read for Input {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Input::File(f) => f.read(buf),
            Input::Cached(c) => c.read(buf),
            Input::Http(h) => h.read(buf),
        }
    }
}

impl Seek for Input {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        match self {
            Input::File(f) => f.seek(pos),
            Input::Cached(c) => c.seek(pos),
            Input::Http(h) => h.seek(pos),
        }
    }
}

/// Open a stream and prepare a decoder for it. This blocks on file or
/// network I/O, so the engine calls it off its control thread.
/// Open a stream for decoding. `cache` keeps a network stream for offline
/// listening once it has downloaded in full.
pub(crate) fn open(
    stream: &Stream,
    cache: Option<CacheWriter>,
) -> Result<Decoder<Input>, AudioError> {
    let (input, len, seekable, hint, mime) = match &stream.location {
        StreamLocation::File(path) => {
            let file = File::open(path)?;
            let len = file.metadata()?.len();
            (Input::File(file), Some(len), true, extension(path), None)
        }
        StreamLocation::Cached(path) => {
            let reader = CachedReader::open(path)?;
            let len = reader.len();
            (Input::Cached(reader), Some(len), true, None, None)
        }
        StreamLocation::Http { url, headers } => {
            let reader = PrefetchReader::new(HttpReader::open(url, headers)?, cache);
            let path = url.split(['?', '#']).next().unwrap_or(url);
            let (len, seekable) = (reader.byte_len(), reader.is_seekable());
            let mime = reader.content_type().map(str::to_owned);
            (
                Input::Http(reader),
                len,
                seekable,
                extension(Path::new(path)),
                mime,
            )
        }
    };

    let mut builder = Decoder::builder()
        .with_data(input)
        .with_seekable(seekable)
        .with_gapless(true);
    if let Some(len) = len {
        builder = builder.with_byte_len(len);
    }
    if let Some(hint) = &hint {
        builder = builder.with_hint(hint);
    }
    // Generic types like application/octet-stream don't help the probe.
    if let Some(mime) = stream
        .mime
        .as_ref()
        .or(mime.as_ref())
        .filter(|m| m.starts_with("audio/"))
    {
        builder = builder.with_mime_type(mime);
    }
    builder
        .build()
        .map_err(|e| AudioError::Decode(e.to_string()))
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rodio::Source;
    use std::time::Duration;

    /// One second of a 440 Hz sine wave as a 16-bit mono WAV file.
    fn wav() -> Vec<u8> {
        let rate = 8000u32;
        let samples: Vec<i16> = (0..rate)
            .map(|i| {
                ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 10000.0) as i16
            })
            .collect();
        let data_len = samples.len() as u32 * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for s in samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    #[test]
    fn decodes_local_file() {
        let path = std::env::temp_dir().join(format!("veena-test-{}.wav", std::process::id()));
        std::fs::write(&path, wav()).unwrap();
        let mut decoder = open(&Stream::file(&path), None).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(decoder.sample_rate().get(), 8000);
        assert_eq!(decoder.channels().get(), 1);
        let duration = decoder.total_duration().unwrap();
        assert!((duration.as_secs_f32() - 1.0).abs() < 0.01, "{duration:?}");
        decoder.try_seek(Duration::from_millis(500)).unwrap();
        assert_eq!(decoder.count(), 4000);
    }

    #[test]
    fn rejects_non_audio() {
        let path = std::env::temp_dir().join(format!("veena-test-{}.txt", std::process::id()));
        std::fs::write(&path, b"not audio").unwrap();
        let result = open(&Stream::file(&path), None);
        std::fs::remove_file(&path).unwrap();
        assert!(matches!(result, Err(AudioError::Decode(_))));
    }
}

#[cfg(test)]
mod cached_file_tests {
    use super::*;
    use crate::AudioCache;
    use rodio::Source;
    use veena_core::{Track, TrackId};

    /// Run with VEENA_TEST_M4A=/path/to/song.m4a to check a real song plays
    /// back from the offline cache.
    #[test]
    fn a_real_song_decodes_from_the_cache() {
        let Some(path) = std::env::var_os("VEENA_TEST_M4A") else {
            return;
        };
        let data = std::fs::read(path).unwrap();
        let dir = std::env::temp_dir().join(format!("veena-decode-cache-{}", std::process::id()));
        let cache = AudioCache::open(&dir, 1 << 30);
        let track = Track::new(TrackId::new("test", "real"), "Real");
        let mut writer = cache.writer(&track, Some("audio/mp4".into())).unwrap();
        for (i, part) in data.chunks(64 * 1024).enumerate() {
            writer.write_at((i * 64 * 1024) as u64, part);
        }
        writer.finish(Some(data.len() as u64));
        let stream = cache.lookup(&track).expect("kept");
        let decoder = open(&stream, None).expect("opens");
        eprintln!(
            "rate {} channels {} duration {:?}",
            decoder.sample_rate(),
            decoder.channels(),
            decoder.total_duration()
        );
        let samples = decoder.take(48_000 * 4).count();
        eprintln!("decoded {samples} samples");
        assert!(samples > 48_000);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(test)]
mod network_tests {
    use super::*;
    use rodio::Source;

    /// Run with VEENA_TEST_URL=<audio link> to check a real network stream
    /// decodes through the read-ahead buffer.
    #[test]
    fn a_real_stream_decodes() {
        let Some(url) = std::env::var("VEENA_TEST_URL").ok() else {
            return;
        };
        let stream = Stream::http(url);
        let started = std::time::Instant::now();
        let dir = std::env::temp_dir().join(format!("veena-net-cache-{}", std::process::id()));
        let cache = crate::AudioCache::open(&dir, 1 << 30);
        let track = veena_core::Track::new(veena_core::TrackId::new("test", "net"), "Net");
        let decoder = open(&stream, cache.writer(&track, Some("audio/mp4".into()))).expect("opens");
        eprintln!(
            "opened in {:?}: rate {} channels {} duration {:?}",
            started.elapsed(),
            decoder.sample_rate(),
            decoder.channels(),
            decoder.total_duration()
        );
        let samples = decoder.take(44_100 * 2 * 10).count();
        eprintln!(
            "decoded {samples} samples in {:?}; kept: {}",
            started.elapsed(),
            cache.contains(&track)
        );
        assert!(samples > 44_100);
        std::thread::sleep(std::time::Duration::from_secs(3));
        eprintln!("kept after 3 s: {}", cache.contains(&track));
        if let Some(cached) = cache.lookup(&track) {
            let n = open(&cached, None)
                .expect("cached opens")
                .take(44_100 * 2 * 10)
                .count();
            eprintln!("decoded {n} samples from the cache");
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
