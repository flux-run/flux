use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

/// Caches function bundles locally to prevent redundant S3/HTTP downloads
/// Key: `deployment_id`
/// Value: Bundle JS code
#[derive(Clone)]
pub struct BundleCache {
    cache: Arc<Mutex<LruCache<String, String>>>,
}

impl BundleCache {
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(100).unwrap());
        Self {
            cache: Arc::new(Mutex::new(LruCache::new(cap))),
        }
    }

    pub fn get(&self, deployment_id: &str) -> Option<String> {
        let mut cache = self.cache.lock().unwrap();
        cache.get(deployment_id).cloned()
    }

    pub fn insert(&self, deployment_id: String, code: String) {
        let mut cache = self.cache.lock().unwrap();
        cache.put(deployment_id, code);
    }
}
