use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{PluginSettings, UpdateError};

/// What the updater has installed, kept in `state.json` in the plugin folder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct State {
    /// Highest manifest sequence applied.
    #[serde(default)]
    pub sequence: u64,
    /// Unix time of the last successful check.
    #[serde(default)]
    pub checked_at: u64,
    #[serde(default)]
    pub plugins: BTreeMap<String, InstalledPlugin>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InstalledPlugin {
    pub version: String,
    /// Version kept in `.previous/` for rollback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous: Option<String>,
    #[serde(flatten)]
    pub settings: PluginSettings,
    /// Versions that failed or were rolled back; never installed again.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected: Vec<String>,
}

impl State {
    pub const FILE: &'static str = "state.json";

    pub fn load(dir: &Path) -> Result<Self, UpdateError> {
        let path = dir.join(Self::FILE);
        match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                UpdateError::BadManifest(format!("unreadable {}: {e}", path.display()))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(UpdateError::io(path)(e)),
        }
    }

    /// Write atomically: a crash never leaves a half-written file.
    pub fn save(&self, dir: &Path) -> Result<(), UpdateError> {
        let json = serde_json::to_vec_pretty(self).expect("state serializes");
        write_atomic(&dir.join(Self::FILE), &json)
    }
}

/// Write to a temporary file in the same folder, then rename over the target.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), UpdateError> {
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(UpdateError::io(dir))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(UpdateError::io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(UpdateError::io(path))
}
