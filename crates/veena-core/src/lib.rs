//! Core types shared by every part of Veena.
//!
//! This crate has no knowledge of audio output or user interfaces. It defines
//! what a track is, how sources provide tracks, and how the play queue behaves.

mod error;
mod queue;
mod registry;
mod source;
mod track;
mod url_source;

pub use error::{Error, Result};
pub use queue::{Queue, Repeat};
pub use registry::SourceRegistry;
pub use source::Source;
pub use track::{Stream, StreamLocation, Track, TrackId};
pub use url_source::UrlSource;

/// Re-exported so source implementations use the same macro version.
pub use async_trait::async_trait;
