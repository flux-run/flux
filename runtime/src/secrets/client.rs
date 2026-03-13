use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use lru::LruCache;
use uuid::Uuid;
use job_contract::dispatch::ApiDispatch;

// ── Cache ─────────────────────────────────────────────────────────────────────

/// Cache entry: (secrets_map, inserted_at)
type CacheEntry = (HashMap<String, String>, Instant);

/// LRU + TTL cache for project secrets.
///
/// Key: `"<project_id>"` or `"none"` (no project)
/// TTL: 30s — secrets rarely change between invocations
#[derive(Clone)]
pub struct SecretsCache {
    inner: Arc<Mutex<LruCache<String, CacheEntry>>>,
    ttl:   Duration,
}

impl SecretsCache {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(50).unwrap());
        Self { inner: Arc::new(Mutex::new(LruCache::new(cap))), ttl }
    }

    pub fn cache_key(project_id: Option<Uuid>) -> String {
        project_id.map(|p| p.to_string()).unwrap_or_else(|| "none".to_string())
    }

    fn get(&self, key: &str) -> Option<HashMap<String, String>> {
        let mut c = self.inner.lock().unwrap();
        match c.get(key) {
            Some((secrets, inserted_at)) if inserted_at.elapsed() < self.ttl => {
                Some(secrets.clone())
            }
            Some(_) => { c.pop(key); None }
            None    => None,
        }
    }

    fn insert(&self, key: String, secrets: HashMap<String, String>) {
        self.inner.lock().unwrap().put(key, (secrets, Instant::now()));
    }

    pub fn invalidate(&self, project_id: Option<Uuid>) {
        let key = Self::cache_key(project_id);
        self.inner.lock().unwrap().pop(&key);
    }
}

// ── Client ────────────────────────────────────────────────────────────────────

/// Secrets client with a built-in LRU+TTL cache.
///
/// Delegates actual network/in-process fetching to an `Arc<dyn ApiDispatch>`
/// so it works in both multi-process mode (HTTP) and server mode (in-process).
#[derive(Clone)]
pub struct SecretsClient {
    api:   Arc<dyn ApiDispatch>,
    cache: SecretsCache,
}

impl SecretsClient {
    pub fn new(api: Arc<dyn ApiDispatch>) -> Self {
        Self {
            api,
            cache: SecretsCache::new(50, Duration::from_secs(30)),
        }
    }

    pub fn cache(&self) -> &SecretsCache { &self.cache }

    /// Fetch secrets for a project — no tenant concept.
    ///
    /// Fast path: serve from in-process LRU cache (avoids ~5 ms control-plane RTT).
    pub async fn fetch_secrets(
        &self,
        project_id: Option<Uuid>,
    ) -> Result<HashMap<String, String>, String> {
        let key = SecretsCache::cache_key(project_id);

        if let Some(cached) = self.cache.get(&key) {
            tracing::debug!(project_id = ?project_id, "secrets cache hit");
            return Ok(cached);
        }

        let secrets_map = self.api.get_secrets(project_id).await?;
        self.cache.insert(key, secrets_map.clone());
        Ok(secrets_map)
    }
}
