use std::sync::{Arc, RwLock};

use crate::{Error, Result, Source, Stream, Track};

/// The set of sources the player can use, looked up by id.
///
/// Cloning gives another handle to the same set, so a source registered or
/// replaced through one handle (for example, an updated plugin) is seen by
/// the player immediately.
#[derive(Clone, Default)]
pub struct SourceRegistry {
    sources: Arc<RwLock<Vec<Arc<dyn Source>>>>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a source. A source with the same id replaces the old one, which is
    /// how updated plugins are swapped in.
    pub fn register(&self, source: Arc<dyn Source>) {
        let mut sources = self.sources.write().unwrap();
        sources.retain(|s| s.id() != source.id());
        sources.push(source);
    }

    /// Remove a source. Returns it if it was registered.
    pub fn unregister(&self, id: &str) -> Option<Arc<dyn Source>> {
        let mut sources = self.sources.write().unwrap();
        let index = sources.iter().position(|s| s.id() == id)?;
        Some(sources.remove(index))
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Source>> {
        self.sources
            .read()
            .unwrap()
            .iter()
            .find(|s| s.id() == id)
            .cloned()
    }

    /// All registered sources, in registration order.
    pub fn sources(&self) -> Vec<Arc<dyn Source>> {
        self.sources.read().unwrap().clone()
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
        let sources = self.sources();
        let mut results = Vec::with_capacity(sources.len());
        for source in sources {
            results.push((source.id().to_owned(), source.search(query).await));
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TrackId, async_trait};

    struct Fake(&'static str, &'static str);

    #[async_trait]
    impl Source for Fake {
        fn id(&self) -> &str {
            self.0
        }
        fn name(&self) -> &str {
            self.1
        }
        async fn search(&self, _: &str) -> Result<Vec<Track>> {
            Ok(vec![])
        }
        async fn resolve(&self, track: &Track) -> Result<Vec<Stream>> {
            Ok(vec![Stream::http(format!(
                "https://{}/{}",
                self.1, track.id.id
            ))])
        }
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        use std::task::{Context, Poll, Waker};
        let mut future = std::pin::pin!(future);
        let mut cx = Context::from_waker(Waker::noop());
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut cx) {
                return output;
            }
        }
    }

    #[test]
    fn replacing_a_source_is_seen_by_every_handle() {
        let registry = SourceRegistry::new();
        let player_handle = registry.clone();
        registry.register(Arc::new(Fake("yt", "v1")));
        registry.register(Arc::new(Fake("yt", "v2")));
        assert_eq!(player_handle.sources().len(), 1);

        let track = Track::new(TrackId::new("yt", "abc"), "t");
        let streams = block_on(player_handle.resolve(&track)).unwrap();
        assert_eq!(streams[0], Stream::http("https://v2/abc"));

        assert!(registry.unregister("yt").is_some());
        assert!(matches!(
            block_on(player_handle.resolve(&track)),
            Err(Error::UnknownSource(_))
        ));
    }
}
