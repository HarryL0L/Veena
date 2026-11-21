//! Core types shared by every part of Veena.
//!
//! This crate has no knowledge of audio output or user interfaces. It defines
//! what a track is, the two brick slots ([`Catalog`] finds music,
//! [`Resolver`] turns it into playable streams), and how the queue behaves.

mod brick;
mod error;
mod lyrics;
mod queue;
mod registry;
mod track;
mod url_source;

pub use brick::{Catalog, Resolver};
pub use error::{Error, Result};
pub use lyrics::{LyricLine, LyricWord, Lyrics};
pub use queue::{Queue, Repeat};
pub use registry::SourceRegistry;
pub use track::{Stream, StreamLocation, Track, TrackId};
pub use url_source::UrlSource;

/// Re-exported so source implementations use the same macro version.
pub use async_trait::async_trait;
