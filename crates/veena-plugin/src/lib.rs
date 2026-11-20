//! JavaScript plugins for Veena: the bricks that update without an app update.
//!
//! A plugin is one JavaScript module. Exporting `search` fills the search
//! slot ([`veena_core::Catalog`]); exporting `resolve` fills the audio-link
//! slot ([`veena_core::Resolver`]). A plugin may fill either or both.
//!
//! ```js
//! export const meta = {
//!   id: "example", name: "Example", version: "1.0.0", api: 1,
//!   kinds: ["example"],                // content kinds it labels or plays
//!   domains: ["example.com"],          // the only hosts it may contact
//! };
//! export async function search(query) {
//!   const res = await veena.http.fetch(`https://example.com/find?q=${encodeURIComponent(query)}`);
//!   return res.json().items.map(i => ({ id: i.id, title: i.name, artist: i.by }));
//! }
//! export async function resolve(track) {
//!   return [{ url: `https://example.com/audio/${track.id}.m4a`, mime: "audio/mp4" }];
//! }
//! ```
//!
//! Each plugin runs in its own Boa JavaScript engine on its own thread. It
//! cannot read files, import code or open connections; it can only call the
//! host API (`veena.http`, `veena.cache`, `veena.config`, `veena.log`), and
//! network access is limited to the domains it declares.

mod brick;
mod convert;
mod host;
mod runtime;

use std::time::Duration;

use serde::{Deserialize, Serialize};

pub use brick::{PluginBrick, load_dir};

/// Version of the host API plugins are written against. A plugin must
/// declare the same `api` number to be loaded.
pub const API_VERSION: u32 = 1;

/// Ids and kinds used by built-in bricks, which plugins may not take over.
pub const RESERVED_IDS: &[&str] = &["local", "url", "file"];

/// The `meta` object a plugin exports.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginMeta {
    /// Unique id: lowercase letters, digits and hyphens.
    pub id: String,
    pub name: String,
    pub version: String,
    /// Host API version the plugin targets.
    pub api: u32,
    /// Content kinds the plugin labels tracks with (search) or can play
    /// (resolve). Defaults to the plugin id.
    #[serde(default)]
    pub kinds: Vec<String>,
    /// Hosts the plugin may contact. Subdomains are included.
    #[serde(default)]
    pub domains: Vec<String>,
    /// Higher is tried first among audio-link bricks for the same kind.
    #[serde(default)]
    pub priority: i32,
}

impl PluginMeta {
    /// Content kinds, defaulting to the plugin id.
    pub fn kinds(&self) -> Vec<String> {
        if self.kinds.is_empty() {
            vec![self.id.clone()]
        } else {
            self.kinds.clone()
        }
    }

    fn validate(&self) -> Result<(), PluginError> {
        let names = std::iter::once(("id", &self.id)).chain(self.kinds.iter().map(|k| ("kind", k)));
        for (what, name) in names {
            if !is_valid_name(name) {
                return Err(PluginError::InvalidMeta(format!(
                    "{what} `{name}` must be 1-64 lowercase letters, digits or hyphens"
                )));
            }
            if RESERVED_IDS.contains(&name.as_str()) {
                return Err(PluginError::InvalidMeta(format!(
                    "{what} `{name}` is reserved"
                )));
            }
        }
        if self.api != API_VERSION {
            return Err(PluginError::IncompatibleApi {
                plugin: self.api,
                host: API_VERSION,
            });
        }
        if let Some(bad) = self.domains.iter().find(|d| !is_valid_domain(d)) {
            return Err(PluginError::InvalidMeta(format!("invalid domain `{bad}`")));
        }
        Ok(())
    }
}

/// An id or kind: lowercase letters, digits and hyphens.
pub(crate) fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// A bare host name such as `example.com` (no scheme, path or wildcard).
fn is_valid_domain(domain: &str) -> bool {
    !domain.is_empty()
        && domain.len() <= 253
        && domain.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

/// Resource limits for one plugin.
#[derive(Clone, Debug)]
pub struct Limits {
    /// Longest a single call (search, resolve, ...) may take.
    pub call_timeout: Duration,
    /// Most iterations any single loop may run.
    pub loop_iterations: u64,
    /// Deepest function recursion allowed.
    pub recursion: usize,
    /// Most HTTP requests a single call may make.
    pub requests_per_call: u32,
    /// Largest HTTP response body a plugin may read.
    pub max_response_bytes: u64,
    /// Timeout for each HTTP request.
    pub request_timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            call_timeout: Duration::from_secs(30),
            loop_iterations: 10_000_000,
            recursion: 400,
            requests_per_call: 32,
            max_response_bytes: 16 * 1024 * 1024,
            request_timeout: Duration::from_secs(20),
        }
    }
}

/// How to load a plugin.
#[derive(Clone, Debug, Default)]
pub struct Options {
    pub limits: Limits,
    /// Values exposed to the plugin as `veena.config`. Lets small fixes be
    /// shipped as configuration instead of a new plugin version.
    pub config: serde_json::Map<String, serde_json::Value>,
    /// Allowed domains from a trusted source (the signed manifest), used
    /// instead of the ones the plugin declares.
    pub domains: Option<Vec<String>>,
}

/// Errors from loading or calling a plugin.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error("plugin failed to load: {0}")]
    Load(String),

    #[error("invalid plugin meta: {0}")]
    InvalidMeta(String),

    #[error("plugin targets API version {plugin}, but this app supports version {host}")]
    IncompatibleApi { plugin: u32, host: u32 },

    #[error("plugin does not export `{0}`")]
    MissingExport(&'static str),

    #[error("plugin exports neither `search` nor `resolve`")]
    NothingProvided,

    #[error("plugin error: {0}")]
    Script(String),

    #[error("plugin returned invalid data: {0}")]
    InvalidResult(String),

    #[error("plugin took longer than {0:?}")]
    Timeout(Duration),

    #[error("plugin stopped unexpectedly")]
    Crashed,
}
