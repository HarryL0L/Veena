//! Signed, automatic updates for Veena's plugin bricks.
//!
//! A publisher lists the current bricks in a manifest and signs it with a
//! private key. Veena checks the signature against a public key built into
//! the app, downloads each changed brick, checks it against the SHA-256 hash
//! in the signed manifest, test-loads it, and swaps it in. The previous
//! version is kept for rollback.
//!
//! ```text
//! manifest.json       {"format":1,"sequence":7,"plugins":[{"id":…,"version":…,"url":…,"sha256":…}]}
//! manifest.json.sig   base64 Ed25519 signature of the exact bytes of manifest.json
//! ```

mod install;
mod keys;
mod manifest;
mod state;

use std::path::PathBuf;

pub use install::{Report, Updater, load_installed};
pub use keys::{generate_signing_key, parse_public_key, parse_signing_key, public_key_of, sign};
pub use manifest::{Manifest, ManifestEntry, PluginSettings, compare_versions, sha256_hex};
pub use state::{InstalledPlugin, State};

/// Version of the manifest format this app reads.
pub const MANIFEST_FORMAT: u32 = 1;

/// Errors from checking for or applying updates.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("network error: {0}")]
    Network(String),

    #[error("the manifest signature is invalid")]
    BadSignature,

    #[error("invalid key: {0}")]
    BadKey(String),

    #[error("invalid manifest: {0}")]
    BadManifest(String),

    #[error(
        "manifest sequence {found} is older than {seen}, which was already applied; refusing a possible replay"
    )]
    Replay { found: u64, seen: u64 },

    #[error("no previous version of `{0}` to roll back to")]
    NothingToRollBack(String),

    #[error("file error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

impl UpdateError {
    fn io(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Self {
        let path = path.into();
        move |source| Self::Io { path, source }
    }
}
