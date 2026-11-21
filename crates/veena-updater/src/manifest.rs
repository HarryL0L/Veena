use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{MANIFEST_FORMAT, UpdateError};

/// The signed list of current bricks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub format: u32,
    /// Increases with every publication; older manifests are refused.
    pub sequence: u64,
    pub plugins: Vec<ManifestEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub version: String,
    /// Plugin host API version it needs.
    pub api: u32,
    /// Download location; relative URLs are resolved against the manifest.
    pub url: String,
    /// Hex SHA-256 of the file.
    pub sha256: String,
    #[serde(flatten)]
    pub settings: PluginSettings,
}

/// Settings delivered with a brick, applied without a new brick version.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PluginSettings {
    /// Exposed to the brick as `veena.config`.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub config: serde_json::Map<String, serde_json::Value>,
    /// Trusted allowed hosts, replacing the ones the brick declares.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domains: Option<Vec<String>>,
}

impl Manifest {
    pub fn parse(bytes: &[u8]) -> Result<Self, UpdateError> {
        let manifest: Manifest =
            serde_json::from_slice(bytes).map_err(|e| UpdateError::BadManifest(e.to_string()))?;
        if manifest.format != MANIFEST_FORMAT {
            return Err(UpdateError::BadManifest(format!(
                "format {} is not supported (this app reads format {MANIFEST_FORMAT})",
                manifest.format
            )));
        }
        let mut ids: Vec<&str> = manifest.plugins.iter().map(|p| p.id.as_str()).collect();
        ids.sort_unstable();
        if let Some(pair) = ids.windows(2).find(|w| w[0] == w[1]) {
            return Err(UpdateError::BadManifest(format!(
                "`{}` is listed twice",
                pair[0]
            )));
        }
        for entry in &manifest.plugins {
            let id_ok = !entry.id.is_empty()
                && entry
                    .id
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
            if !id_ok {
                return Err(UpdateError::BadManifest(format!(
                    "invalid id `{}`",
                    entry.id
                )));
            }
            if entry.sha256.len() != 64 || !entry.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(UpdateError::BadManifest(format!(
                    "invalid sha256 for `{}`",
                    entry.id
                )));
            }
        }
        Ok(manifest)
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Compare dotted numeric versions ("1.10.0" > "1.9.2"). Non-numeric parts
/// compare as text.
pub(crate) fn compare_versions(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> Vec<String> { v.split(['.', '-', '+']).map(str::to_owned).collect() };
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).map_or("0", String::as_str),
            b.get(i).map_or("0", String::as_str),
        );
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

/// Resolve a possibly relative URL against the manifest's URL.
pub(crate) fn join_url(manifest_url: &str, url: &str) -> String {
    if url.contains("://") {
        return url.to_owned();
    }
    let base = manifest_url
        .split(['?', '#'])
        .next()
        .unwrap_or(manifest_url);
    match base.rfind('/') {
        Some(slash) => format!("{}/{}", &base[..slash], url.trim_start_matches("./")),
        None => url.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(compare_versions("1.10.0", "1.9.2"), Ordering::Greater);
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Equal);
        assert_eq!(compare_versions("0.1.1", "0.2.0"), Ordering::Less);
    }

    #[test]
    fn urls() {
        assert_eq!(
            join_url("https://a.org/x/manifest.json", "b.js"),
            "https://a.org/x/b.js"
        );
        assert_eq!(
            join_url("https://a.org/x/manifest.json", "https://c.org/d.js"),
            "https://c.org/d.js"
        );
    }

    #[test]
    fn rejects_bad_manifests() {
        let entry = r#"{"id":"a","version":"1","api":1,"url":"a.js","sha256":"0000000000000000000000000000000000000000000000000000000000000000"}"#;
        assert!(
            Manifest::parse(
                format!(r#"{{"format":1,"sequence":1,"plugins":[{entry}]}}"#).as_bytes()
            )
            .is_ok()
        );
        assert!(
            Manifest::parse(
                format!(r#"{{"format":2,"sequence":1,"plugins":[{entry}]}}"#).as_bytes()
            )
            .is_err()
        );
        assert!(
            Manifest::parse(
                format!(r#"{{"format":1,"sequence":1,"plugins":[{entry},{entry}]}}"#).as_bytes()
            )
            .is_err()
        );
        assert!(Manifest::parse(br#"{"format":1,"sequence":1,"plugins":[{"id":"A!","version":"1","api":1,"url":"a","sha256":"00"}]}"#).is_err());
    }
}
