//! A loaded plugin, plugged into the search and/or audio-link slots.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

use serde_json::Value;
use tokio::sync::oneshot;
use veena_core::{
    Catalog, Error, Resolver, Result, SourceRegistry, Stream, StreamLocation, Track, async_trait,
};
use veena_net::ureq::http;

use crate::runtime::{self, Job, Slots};
use crate::{Options, PluginError, PluginMeta, convert, host};

/// A JavaScript plugin brick.
///
/// Calls are sent to the plugin's thread. If a call times out or the engine
/// crashes, the plugin is restarted from its code on the next call.
pub struct PluginBrick {
    meta: PluginMeta,
    slots: Slots,
    code: Arc<str>,
    options: Arc<Options>,
    worker: Mutex<Option<mpsc::Sender<Job>>>,
}

impl PluginBrick {
    /// Evaluate plugin code and check its `meta`.
    pub async fn load(code: impl Into<Arc<str>>, options: Options) -> Result<Self, PluginError> {
        let code = code.into();
        let options = Arc::new(options);
        let (worker, meta, slots) = start(&code, &options).await?;
        Ok(Self {
            meta,
            slots,
            code,
            options,
            worker: Mutex::new(Some(worker)),
        })
    }

    /// Load a plugin from a `.js` file.
    pub async fn load_file(path: &Path, options: Options) -> Result<Self, PluginError> {
        let code = std::fs::read_to_string(path)?;
        Self::load(code, options).await
    }

    pub fn meta(&self) -> &PluginMeta {
        &self.meta
    }

    /// Whether the plugin fills the search slot (exports `search`).
    pub fn is_catalog(&self) -> bool {
        self.slots.catalog
    }

    /// Whether the plugin fills the audio-link slot (exports `resolve`).
    pub fn is_resolver(&self) -> bool {
        self.slots.resolver
    }

    /// Whether the plugin finds lyrics (exports `lyrics`).
    pub fn is_lyrics(&self) -> bool {
        self.slots.lyrics
    }

    /// Lyrics for a track, or `None` if the brick found none.
    pub async fn lyrics(&self, track: &Track) -> Result<Option<veena_core::Lyrics>> {
        let value = self
            .call("lyrics", vec![convert::track_to_js(track)])
            .await
            .map_err(|e| self.error(e))?;
        convert::lyrics(value, &self.meta.name).map_err(|e| self.error(e))
    }

    /// Domains the plugin may contact.
    pub fn domains(&self) -> &[String] {
        self.options
            .domains
            .as_deref()
            .unwrap_or(&self.meta.domains)
    }

    /// Plug this brick into the slots it fills, replacing an older version
    /// with the same id.
    pub fn install(self: Arc<Self>, registry: &SourceRegistry) {
        let id = self.meta.id.clone();
        registry.remove_catalog(&id);
        if self.is_catalog() {
            registry.add_catalog(self.clone());
        }
        if self.is_resolver() {
            registry.add_resolver(self);
        } else {
            registry.remove_resolver(&id);
        }
    }

    async fn call(&self, function: &'static str, args: Vec<Value>) -> Result<Value, PluginError> {
        let worker = self.worker().await?;
        let (reply, result) = oneshot::channel();
        if worker
            .send(Job {
                function,
                args,
                reply,
            })
            .is_err()
        {
            self.reset();
            return Err(PluginError::Crashed);
        }
        let timeout = self.options.limits.call_timeout;
        match tokio::time::timeout(timeout, result).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => {
                self.reset();
                Err(PluginError::Crashed)
            }
            Err(_) => {
                // The stuck thread is abandoned; it stops at the loop limit
                // or when its current request times out.
                self.reset();
                Err(PluginError::Timeout(timeout))
            }
        }
    }

    /// The running worker, restarting it if needed.
    async fn worker(&self) -> Result<mpsc::Sender<Job>, PluginError> {
        if let Some(worker) = self.worker.lock().unwrap().clone() {
            return Ok(worker);
        }
        let (worker, _, _) = start(&self.code, &self.options).await?;
        *self.worker.lock().unwrap() = Some(worker.clone());
        Ok(worker)
    }

    fn reset(&self) {
        *self.worker.lock().unwrap() = None;
    }

    fn error(&self, error: PluginError) -> Error {
        match error {
            PluginError::MissingExport(name) => Error::Unsupported {
                source_id: self.meta.id.clone(),
                operation: name,
            },
            other => Error::Source(format!("{}: {other}", self.meta.id)),
        }
    }

    fn default_kind(&self) -> String {
        self.meta.kinds().swap_remove(0)
    }

    /// Streams must point at allowed domains too, so a plugin cannot make
    /// the player contact other hosts.
    fn check_streams(&self, streams: &[Stream]) -> Result<(), PluginError> {
        for stream in streams {
            if let StreamLocation::Http { url, .. } = &stream.location {
                let uri: http::Uri = url.parse().map_err(|_| {
                    PluginError::InvalidResult(format!("invalid stream URL: {url}"))
                })?;
                host::check_allowed(&uri, self.domains()).map_err(PluginError::InvalidResult)?;
            }
        }
        Ok(())
    }
}

async fn start(
    code: &Arc<str>,
    options: &Arc<Options>,
) -> Result<(mpsc::Sender<Job>, PluginMeta, Slots), PluginError> {
    let (ready_tx, ready) = oneshot::channel();
    let worker = runtime::spawn(code.clone(), options.clone(), ready_tx)?;
    let timeout = options.limits.call_timeout;
    let (meta, slots) = match tokio::time::timeout(timeout, ready).await {
        Ok(Ok(result)) => result?,
        Ok(Err(_)) => return Err(PluginError::Crashed),
        Err(_) => return Err(PluginError::Timeout(timeout)),
    };
    Ok((worker, meta, slots))
}

#[async_trait]
impl Catalog for PluginBrick {
    fn id(&self) -> &str {
        &self.meta.id
    }

    fn name(&self) -> &str {
        &self.meta.name
    }

    async fn search(&self, query: &str) -> Result<Vec<Track>> {
        let value = self
            .call("search", vec![Value::from(query)])
            .await
            .map_err(|e| self.error(e))?;
        convert::tracks(&self.default_kind(), value).map_err(|e| self.error(e))
    }

    async fn playlist(&self, id: &str) -> Result<Vec<Track>> {
        let value = self
            .call("playlist", vec![Value::from(id)])
            .await
            .map_err(|e| self.error(e))?;
        convert::tracks(&self.default_kind(), value).map_err(|e| self.error(e))
    }
    async fn related(&self, track: &Track) -> Result<Vec<Track>> {
        let value = self
            .call("related", vec![convert::track_to_js(track)])
            .await
            .map_err(|e| self.error(e))?;
        convert::tracks(&self.default_kind(), value).map_err(|e| self.error(e))
    }
}

#[async_trait]
impl Resolver for PluginBrick {
    fn id(&self) -> &str {
        &self.meta.id
    }

    fn name(&self) -> &str {
        &self.meta.name
    }

    fn kinds(&self) -> Vec<String> {
        self.meta.kinds()
    }

    async fn resolve(&self, track: &Track) -> Result<Vec<Stream>> {
        let value = self
            .call("resolve", vec![convert::track_to_js(track)])
            .await
            .map_err(|e| self.error(e))?;
        let streams = convert::streams(value).map_err(|e| self.error(e))?;
        self.check_streams(&streams).map_err(|e| self.error(e))?;
        Ok(streams)
    }
}

/// Load every `.js` file in a folder. Plugins come back highest `priority`
/// first (then by file name), so installing them in order sets the fallback
/// order. Each result is reported separately so one broken plugin does not
/// block the others.
pub async fn load_dir(
    dir: &Path,
    options: &Options,
) -> std::io::Result<Vec<(PathBuf, Result<PluginBrick, PluginError>)>> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|e| e == "js") && path.is_file())
        .collect();
    paths.sort();
    let mut plugins = Vec::with_capacity(paths.len());
    for path in paths {
        let plugin = PluginBrick::load_file(&path, options.clone()).await;
        plugins.push((path, plugin));
    }
    plugins.sort_by_key(|(_, plugin)| {
        std::cmp::Reverse(plugin.as_ref().map_or(i32::MIN, |p| p.meta.priority))
    });
    Ok(plugins)
}
