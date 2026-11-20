use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::{Catalog, Error, Resolver, Result, Stream, Track};

/// The installed bricks: catalogs (search) and resolvers (audio links).
///
/// Cloning gives another handle to the same set, so a brick registered or
/// replaced through one handle (for example, an updated plugin) is seen by
/// the player immediately.
#[derive(Clone, Default)]
pub struct SourceRegistry {
    inner: Arc<RwLock<Inner>>,
}

#[derive(Default)]
struct Inner {
    catalogs: Vec<Arc<dyn Catalog>>,
    resolvers: Vec<Arc<dyn Resolver>>,
    /// Preferred resolver order per kind; unlisted resolvers follow in
    /// registration order.
    order: HashMap<String, Vec<String>>,
}

impl SourceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a catalog. One with the same id is replaced.
    pub fn add_catalog(&self, catalog: Arc<dyn Catalog>) {
        let mut inner = self.inner.write().unwrap();
        inner.catalogs.retain(|c| c.id() != catalog.id());
        inner.catalogs.push(catalog);
    }

    /// Add a resolver. One with the same id is replaced in place, keeping
    /// its position in the fallback order.
    pub fn add_resolver(&self, resolver: Arc<dyn Resolver>) {
        let mut inner = self.inner.write().unwrap();
        match inner.resolvers.iter().position(|r| r.id() == resolver.id()) {
            Some(index) => inner.resolvers[index] = resolver,
            None => inner.resolvers.push(resolver),
        }
    }

    pub fn remove_catalog(&self, id: &str) -> bool {
        let mut inner = self.inner.write().unwrap();
        let before = inner.catalogs.len();
        inner.catalogs.retain(|c| c.id() != id);
        before != inner.catalogs.len()
    }

    pub fn remove_resolver(&self, id: &str) -> bool {
        let mut inner = self.inner.write().unwrap();
        let before = inner.resolvers.len();
        inner.resolvers.retain(|r| r.id() != id);
        before != inner.resolvers.len()
    }

    /// Remove every brick with this id. Returns true if any was removed.
    pub fn remove(&self, id: &str) -> bool {
        let mut inner = self.inner.write().unwrap();
        let before = inner.catalogs.len() + inner.resolvers.len();
        inner.catalogs.retain(|c| c.id() != id);
        inner.resolvers.retain(|r| r.id() != id);
        before != inner.catalogs.len() + inner.resolvers.len()
    }

    pub fn catalog(&self, id: &str) -> Option<Arc<dyn Catalog>> {
        let inner = self.inner.read().unwrap();
        inner.catalogs.iter().find(|c| c.id() == id).cloned()
    }

    pub fn catalogs(&self) -> Vec<Arc<dyn Catalog>> {
        self.inner.read().unwrap().catalogs.clone()
    }

    pub fn resolvers(&self) -> Vec<Arc<dyn Resolver>> {
        self.inner.read().unwrap().resolvers.clone()
    }

    /// Set which resolvers to try first for a kind, by id.
    pub fn set_resolver_order(&self, kind: &str, ids: Vec<String>) {
        self.inner
            .write()
            .unwrap()
            .order
            .insert(kind.to_owned(), ids);
    }

    /// Resolvers for a kind, in the order they will be tried.
    pub fn resolvers_for(&self, kind: &str) -> Vec<Arc<dyn Resolver>> {
        let inner = self.inner.read().unwrap();
        let mut matching: Vec<_> = inner
            .resolvers
            .iter()
            .filter(|r| r.kinds().iter().any(|k| k == kind))
            .cloned()
            .collect();
        if let Some(order) = inner.order.get(kind) {
            // Stable sort: listed ids first in list order, the rest after.
            matching.sort_by_key(|r| {
                order
                    .iter()
                    .position(|id| id == r.id())
                    .unwrap_or(usize::MAX)
            });
        }
        matching
    }

    /// Get playable streams for a track, trying each resolver for its kind
    /// in order until one succeeds.
    pub async fn resolve(&self, track: &Track) -> Result<Vec<Stream>> {
        let resolvers = self.resolvers_for(&track.id.kind);
        if resolvers.is_empty() {
            return Err(Error::NoResolver(track.id.kind.clone()));
        }
        let mut attempts = Vec::new();
        for resolver in resolvers {
            match resolver.resolve(track).await {
                Ok(streams) if !streams.is_empty() => return Ok(streams),
                Ok(_) => attempts.push((resolver.id().to_owned(), "no streams".to_owned())),
                Err(e) => attempts.push((resolver.id().to_owned(), e.to_string())),
            }
        }
        if let [(_, only)] = attempts.as_slice() {
            // One resolver: report its error directly.
            return Err(Error::Source(only.clone()));
        }
        Err(Error::AllResolversFailed {
            track: track.id.to_string(),
            attempts,
        })
    }

    /// Search every catalog. Results are returned per catalog so one
    /// failing catalog does not hide results from the others.
    pub async fn search_all(&self, query: &str) -> Vec<(String, Result<Vec<Track>>)> {
        let catalogs = self.catalogs();
        let mut results = Vec::with_capacity(catalogs.len());
        for catalog in catalogs {
            results.push((catalog.id().to_owned(), catalog.search(query).await));
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TrackId, async_trait};

    /// A resolver that succeeds with a URL on `host`, or fails.
    struct Fake {
        id: &'static str,
        host: Option<&'static str>,
    }

    #[async_trait]
    impl Resolver for Fake {
        fn id(&self) -> &str {
            self.id
        }
        fn name(&self) -> &str {
            self.id
        }
        fn kinds(&self) -> Vec<String> {
            vec!["yt".into()]
        }
        async fn resolve(&self, track: &Track) -> Result<Vec<Stream>> {
            match self.host {
                Some(host) => Ok(vec![Stream::http(format!(
                    "https://{host}/{}",
                    track.id.id
                ))]),
                None => Err(Error::Source(format!("{} is broken", self.id))),
            }
        }
    }

    fn fake(id: &'static str, host: Option<&'static str>) -> Arc<dyn Resolver> {
        Arc::new(Fake { id, host })
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

    fn track() -> Track {
        Track::new(TrackId::new("yt", "abc"), "t")
    }

    #[test]
    fn falls_back_to_the_next_resolver() {
        let registry = SourceRegistry::new();
        registry.add_resolver(fake("broken", None));
        registry.add_resolver(fake("working", Some("b")));
        let streams = block_on(registry.resolve(&track())).unwrap();
        assert_eq!(streams[0], Stream::http("https://b/abc"));
    }

    #[test]
    fn reports_every_failure() {
        let registry = SourceRegistry::new();
        registry.add_resolver(fake("one", None));
        registry.add_resolver(fake("two", None));
        let error = block_on(registry.resolve(&track()))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("one: source error: one is broken"),
            "{error}"
        );
        assert!(
            error.contains("two: source error: two is broken"),
            "{error}"
        );
    }

    #[test]
    fn unknown_kind_has_no_resolver() {
        let registry = SourceRegistry::new();
        registry.add_resolver(fake("one", Some("a")));
        let other = Track::new(TrackId::new("soundcloud", "x"), "t");
        assert!(matches!(
            block_on(registry.resolve(&other)),
            Err(Error::NoResolver(_))
        ));
    }

    #[test]
    fn order_can_be_chosen() {
        let registry = SourceRegistry::new();
        registry.add_resolver(fake("first", Some("a")));
        registry.add_resolver(fake("second", Some("b")));
        registry.set_resolver_order("yt", vec!["second".into()]);
        let ids: Vec<String> = registry
            .resolvers_for("yt")
            .iter()
            .map(|r| r.id().to_owned())
            .collect();
        assert_eq!(ids, ["second", "first"]);
    }

    #[test]
    fn updated_brick_is_seen_by_every_handle_and_keeps_its_place() {
        let registry = SourceRegistry::new();
        let player_handle = registry.clone();
        registry.add_resolver(fake("plugin", Some("v1")));
        registry.add_resolver(fake("backup", Some("backup")));
        registry.add_resolver(fake("plugin", Some("v2")));

        let streams = block_on(player_handle.resolve(&track())).unwrap();
        assert_eq!(streams[0], Stream::http("https://v2/abc"));

        assert!(registry.remove("plugin"));
        let streams = block_on(player_handle.resolve(&track())).unwrap();
        assert_eq!(streams[0], Stream::http("https://backup/abc"));
    }
}
