use std::io::Read;
use std::path::{Path, PathBuf};

use ed25519_dalek::VerifyingKey;
use veena_plugin::{API_VERSION, Options, PluginBrick, PluginError};

use crate::manifest::{compare_versions, join_url};
use crate::state::write_atomic;
use crate::{InstalledPlugin, Manifest, State, UpdateError, keys, sha256_hex};

const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 4096;
const MAX_PLUGIN_BYTES: u64 = 8 * 1024 * 1024;
const PREVIOUS_DIR: &str = ".previous";

/// What a check did.
#[derive(Debug, Default)]
pub struct Report {
    /// Sequence of the manifest that was applied.
    pub sequence: u64,
    /// `(id, previous version, new version)`.
    pub installed: Vec<(String, Option<String>, String)>,
    /// Bricks whose settings changed without a new version.
    pub settings_changed: Vec<String>,
    pub unchanged: Vec<String>,
    /// `(id, reason)`: not applicable to this app, or not newer.
    pub skipped: Vec<(String, String)>,
    /// `(id, error)`: a new version that could not be installed.
    pub failed: Vec<(String, String)>,
}

/// Checks a signed manifest and keeps the bricks in a folder up to date.
pub struct Updater {
    dir: PathBuf,
    key: VerifyingKey,
    manifest_url: String,
    options: Options,
}

impl Updater {
    /// `public_key` is the base64 Ed25519 key manifests must be signed with.
    pub fn new(
        dir: impl Into<PathBuf>,
        public_key: &str,
        manifest_url: impl Into<String>,
    ) -> Result<Self, UpdateError> {
        Ok(Self {
            dir: dir.into(),
            key: keys::parse_public_key(public_key)?,
            manifest_url: manifest_url.into(),
            options: Options::default(),
        })
    }

    /// Limits used when test-loading new bricks.
    pub fn with_options(mut self, options: Options) -> Self {
        self.options = options;
        self
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Download the manifest and install every newer brick it lists.
    pub async fn check(&self) -> Result<Report, UpdateError> {
        let url = self.manifest_url.clone();
        let (bytes, signature) = blocking(move || {
            let bytes = download(&url, MAX_MANIFEST_BYTES)?;
            let signature = download(&format!("{url}.sig"), MAX_SIGNATURE_BYTES)?;
            Ok((bytes, String::from_utf8_lossy(&signature).into_owned()))
        })
        .await?;
        keys::verify(&self.key, &bytes, &signature)?;
        let manifest = Manifest::parse(&bytes)?;

        let mut state = State::load(&self.dir)?;
        if manifest.sequence < state.sequence {
            return Err(UpdateError::Replay {
                found: manifest.sequence,
                seen: state.sequence,
            });
        }

        let mut report = Report {
            sequence: manifest.sequence,
            ..Report::default()
        };
        for entry in &manifest.plugins {
            let current = state.plugins.get(&entry.id).cloned();
            if entry.api != API_VERSION {
                report.skipped.push((
                    entry.id.clone(),
                    format!("needs plugin API {}, this app has {API_VERSION}", entry.api),
                ));
                continue;
            }
            if let Some(current) = &current {
                if current.rejected.contains(&entry.version) {
                    report.skipped.push((
                        entry.id.clone(),
                        format!("version {} failed before", entry.version),
                    ));
                    continue;
                }
                if current.version == entry.version {
                    if current.settings != entry.settings {
                        let installed = state.plugins.get_mut(&entry.id).expect("present");
                        installed.settings = entry.settings.clone();
                        report.settings_changed.push(entry.id.clone());
                    } else {
                        report.unchanged.push(entry.id.clone());
                    }
                    continue;
                }
                if compare_versions(&entry.version, &current.version).is_lt() {
                    report.skipped.push((
                        entry.id.clone(),
                        format!(
                            "{} is older than installed {}",
                            entry.version, current.version
                        ),
                    ));
                    continue;
                }
            }

            match self.install(entry, current.as_ref()).await {
                Ok(installed) => {
                    report.installed.push((
                        entry.id.clone(),
                        installed.previous.clone(),
                        entry.version.clone(),
                    ));
                    state.plugins.insert(entry.id.clone(), installed);
                }
                Err(error) => {
                    log::warn!("could not install {} {}: {error}", entry.id, entry.version);
                    let record =
                        state
                            .plugins
                            .entry(entry.id.clone())
                            .or_insert_with(|| InstalledPlugin {
                                version: String::new(),
                                ..InstalledPlugin::default()
                            });
                    record.rejected.push(entry.version.clone());
                    report.failed.push((entry.id.clone(), error));
                }
            }
        }

        state.sequence = manifest.sequence;
        state.checked_at = now();
        state.save(&self.dir)?;
        Ok(report)
    }

    /// Download, verify, test-load and install one brick.
    async fn install(
        &self,
        entry: &crate::ManifestEntry,
        current: Option<&InstalledPlugin>,
    ) -> Result<InstalledPlugin, String> {
        let url = join_url(&self.manifest_url, &entry.url);
        let bytes = blocking(move || download(&url, MAX_PLUGIN_BYTES))
            .await
            .map_err(|e| e.to_string())?;
        let hash = sha256_hex(&bytes);
        if !hash.eq_ignore_ascii_case(&entry.sha256) {
            return Err(format!("download does not match the signed hash ({hash})"));
        }
        let code = String::from_utf8(bytes).map_err(|_| "not UTF-8 text".to_owned())?;

        let mut options = self.options.clone();
        options.config = entry.settings.config.clone();
        options.domains = entry.settings.domains.clone();
        let brick = PluginBrick::load(code.clone(), options)
            .await
            .map_err(|e| format!("does not load: {e}"))?;
        let meta = brick.meta();
        if meta.id != entry.id || meta.version != entry.version {
            return Err(format!(
                "file is {} {}, but the manifest says {} {}",
                meta.id, meta.version, entry.id, entry.version
            ));
        }

        let path = self.dir.join(format!("{}.js", entry.id));
        let previous = if path.exists() {
            let keep = self.dir.join(PREVIOUS_DIR).join(format!("{}.js", entry.id));
            let old = std::fs::read(&path).map_err(|e| e.to_string())?;
            write_atomic(&keep, &old).map_err(|e| e.to_string())?;
            Some(
                current
                    .map(|c| c.version.clone())
                    .filter(|v| !v.is_empty())
                    .unwrap_or_else(|| "local copy".into()),
            )
        } else {
            None
        };
        write_atomic(&path, code.as_bytes()).map_err(|e| e.to_string())?;
        Ok(InstalledPlugin {
            version: entry.version.clone(),
            previous,
            settings: entry.settings.clone(),
            rejected: current.map(|c| c.rejected.clone()).unwrap_or_default(),
        })
    }

    /// Restore the previous version of a brick. The current version is
    /// marked as rejected so it is not installed again.
    pub fn rollback(&self, id: &str) -> Result<String, UpdateError> {
        rollback_in(&self.dir, id)
    }
}

fn rollback_in(dir: &Path, id: &str) -> Result<String, UpdateError> {
    let previous_file = dir.join(PREVIOUS_DIR).join(format!("{id}.js"));
    if !previous_file.exists() {
        return Err(UpdateError::NothingToRollBack(id.into()));
    }
    let mut state = State::load(dir)?;
    let code = std::fs::read(&previous_file).map_err(UpdateError::io(&previous_file))?;
    write_atomic(&dir.join(format!("{id}.js")), &code)?;
    std::fs::remove_file(&previous_file).map_err(UpdateError::io(&previous_file))?;

    let record = state.plugins.entry(id.into()).or_default();
    if !record.version.is_empty() && !record.rejected.contains(&record.version) {
        let bad = record.version.clone();
        record.rejected.push(bad);
    }
    record.version = record.previous.take().unwrap_or_else(|| "previous".into());
    let restored = record.version.clone();
    state.save(dir)?;
    Ok(restored)
}

/// Load every brick in a plugin folder with the settings delivered for it.
///
/// `base` options apply to all; settings from the manifest come first and
/// `base.config` keys override them (so local `--config` wins). A brick
/// that fails to load is rolled back automatically when a previous version
/// exists. Results come back highest `priority` first.
pub async fn load_installed(
    dir: &Path,
    base: &Options,
) -> std::io::Result<Vec<(PathBuf, Result<PluginBrick, PluginError>)>> {
    let state = State::load(dir).unwrap_or_default();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|e| e == "js") && path.is_file())
        .collect();
    paths.sort();

    let mut plugins = Vec::with_capacity(paths.len());
    for path in paths {
        let id = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut options = base.clone();
        if let Some(installed) = state.plugins.get(&id) {
            let mut config = installed.settings.config.clone();
            config.extend(base.config.clone());
            options.config = config;
            options.domains = installed.settings.domains.clone().or(base.domains.clone());
        }
        let mut plugin = PluginBrick::load_file(&path, options.clone()).await;
        if let Err(error) = &plugin
            && dir.join(PREVIOUS_DIR).join(format!("{id}.js")).exists()
        {
            log::warn!("{id} does not load ({error}); rolling back");
            if rollback_in(dir, &id).is_ok() {
                plugin = PluginBrick::load_file(&path, options).await;
            }
        }
        plugins.push((path, plugin));
    }
    plugins.sort_by_key(|(_, plugin)| {
        std::cmp::Reverse(plugin.as_ref().map_or(i32::MIN, |p| p.meta().priority))
    });
    Ok(plugins)
}

/// Fetch a URL, refusing plain http except to this machine, and bodies
/// larger than `limit`.
fn download(url: &str, limit: u64) -> Result<Vec<u8>, UpdateError> {
    let loopback = ["http://127.0.0.1", "http://localhost", "http://[::1]"]
        .iter()
        .any(|prefix| url.starts_with(prefix));
    if !url.starts_with("https://") && !loopback {
        return Err(UpdateError::Network(format!(
            "only https is allowed: {url}"
        )));
    }
    let mut response = veena_net::agent()
        .get(url)
        .call()
        .map_err(|e| UpdateError::Network(format!("{url}: {e}")))?;
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| UpdateError::Network(format!("{url}: {e}")))?;
    if bytes.len() as u64 > limit {
        return Err(UpdateError::Network(format!(
            "{url} is larger than {limit} bytes"
        )));
    }
    Ok(bytes)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, UpdateError> + Send + 'static,
) -> Result<T, UpdateError> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| UpdateError::Network(format!("download task failed: {e}")))?
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
