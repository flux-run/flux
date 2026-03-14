//! Secrets client — fetches and caches project secrets.
//!
//! ## LRU + TTL cache (30 s)
//!
//! Secrets are fetched via `ApiDispatch::get_secrets` which in multi-process mode
//! makes an HTTP call to the control-plane API. To avoid this ~5 ms RTT on every
//! function invocation, results are cached in an in-process LRU (50 entries) with a
//! 30 s TTL.
//!
//! 30 s was chosen to balance:
//! - **Security**: a secret rotation is visible to all running workers within 30 s.
//! - **Performance**: high-throughput functions pay the control-plane cost once per
//!   30 s window, not once per invocation.
//!
//! ## Secret injection
//!
//! Secrets are injected into V8 via `OpState` before the function executes, and into
//! WASM via `HostState` before the module is called. In both cases secrets are only
//! in memory for the duration of the execution — they are not logged or serialised.
//!
//! ## DIP
//!
//! `SecretsClient` depends on `Arc<dyn ApiDispatch>`, not on any HTTP client directly.
//! In server mode the in-process implementation reads secrets directly from the DB —
//! zero network hop.
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use lru::LruCache;
use job_contract::dispatch::ApiDispatch;

// ── Cache ─────────────────────────────────────────────────────────────────────

/// Cache entry: (secrets_map, inserted_at)
type CacheEntry = (HashMap<String, String>, Instant);

/// LRU + TTL cache for secrets.
///
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

    fn get(&self, key: &str) -> Option<HashMap<String, String>> {
        let mut c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match c.get(key) {
            Some((secrets, inserted_at)) if inserted_at.elapsed() < self.ttl => {
                Some(secrets.clone())
            }
            Some(_) => { c.pop(key); None }
            None    => None,
        }
    }

    fn insert(&self, key: String, secrets: HashMap<String, String>) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).put(key, (secrets, Instant::now()));
    }

    pub fn invalidate(&self) {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).pop("secrets");
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

    /// Fetch secrets — no project scoping in single-tenant mode.
    ///
    /// Fast path: serve from in-process LRU cache (avoids ~5 ms control-plane RTT).
    pub async fn fetch_secrets(
        &self,
    ) -> Result<HashMap<String, String>, String> {
        let key = "secrets".to_string();

        if let Some(cached) = self.cache.get(&key) {
            tracing::debug!("secrets cache hit");
            return Ok(cached);
        }

        let secrets_map = self.api.get_secrets().await?;
        self.cache.insert(key, secrets_map.clone());
        Ok(secrets_map)
    }
}
