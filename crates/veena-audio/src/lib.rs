//! Audio playback for Veena.
//!
//! [`AudioEngine`] plays one [`veena_core::Stream`] at a time from a local
//! file or over HTTP. Decoding is done by symphonia and output by cpal, both
//! through rodio, so the same code runs on Linux and Android.

mod cache;
mod decode;
mod engine;
mod http;
mod prefetch;

pub use cache::{AudioCache, CacheEntry, CacheWriter, CachedReader};
pub use engine::{AudioEngine, AudioEvent, PlaybackState};
pub use http::HttpReader;
pub use prefetch::PrefetchReader;

/// Errors from the audio engine.
#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("could not open audio output: {0}")]
    Output(String),

    #[error("could not decode audio: {0}")]
    Decode(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}
