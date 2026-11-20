use std::sync::Arc;

use crate::{Error, Result, Source, Stream, Track};

/// The set of sources the player can use, looked up by id.
#[derive(Clone, Default)]
pub struct SourceRegistry {
    sources: Vec<Arc<dyn Source>>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a source. A source with the same id replaces the old one, which is
    /// how updated plugins will be swapped in.
    pub fn register(&mut self, source: Arc<dyn Source>) {
        self.sources.retain(|s| s.id() != source.id());
        self.sources.push(source);
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn Source>> {
        self.sources.iter().find(|s| s.id() == id)
    }

    pub fn sources(&self) -> &[Arc<dyn Source>] {
        &self.sources
    }

    /// Ask the track's own source for playable streams.
    pub async fn resolve(&self, track: &Track) -> Result<Vec<Stream>> {
        let source = self
            .get(&track.id.source)
            .ok_or_else(|| Error::UnknownSource(track.id.source.clone()))?;
        let streams = source.resolve(track).await?;
        if streams.is_empty() {
            return Err(Error::NoStream(track.id.to_string()));
        }
        Ok(streams)
    }

    /// Search every source. Results are returned per source so one failing
    /// source does not hide results from the others.
    pub async fn search_all(&self, query: &str) -> Vec<(String, Result<Vec<Track>>)> {
        let mut results = Vec::with_capacity(self.sources.len());
        for source in &self.sources {
            results.push((source.id().to_owned(), source.search(query).await));
        }
        results
    }
}
