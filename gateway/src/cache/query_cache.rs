/// In-memory edge cache for read-only data-engine query responses.
///
/// Flow:
///   POST /db/query → cache lookup (project_id + body_hash)
///     HIT  → return cached bytes,  add `X-Cache: HIT`
///     MISS → proxy to data-engine, store result, add `X-Cache: MISS`
///
/// Invalidation:
///   POST /internal/cache/invalidate { project_id, table? }
///     - called by data-engine / API when a write mutation completes
///     - evicts all entries matching (project_id[, table])
///
/// The cache also runs a background task that evicts expired entries every
/// EVICTION_INTERVAL_SECS to bound memory usage.

use bytes::Bytes;
use dashmap::DashMap;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ── Tunables ──────────────────────────────────────────────────────────────

/// Default TTL for a cached entry (seconds).
pub const DEFAULT_TTL_SECS: u64 = 30;
/// How often the background eviction task runs.
const EVICTION_INTERVAL_SECS: u64 = 60;
/// Max number of entries in the cache (very rough memory bound).
const MAX_ENTRIES: usize = 4_096;

// ── Types ─────────────────────────────────────────────────────────────────

/// Cache key: project scope + content-addressed request body.
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct QueryCacheKey {
    /// `X-Fluxbase-Project` header value — isolates tenants.
    pub project_id: String,
    /// SHA-256 of the raw request body bytes.
    pub body_hash: [u8; 32],
}

impl QueryCacheKey {
    pub fn new(project_id: &str, body: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(body);
        let hash: [u8; 32] = hasher.finalize().into();
        Self {
            project_id: project_id.to_string(),
            body_hash: hash,
        }
    }
}

/// A single cached response.
#[derive(Clone)]
pub struct CacheEntry {
    /// Raw response body from the data-engine.
    pub body: Bytes,
    /// HTTP status code (only 2xx responses are stored).
    pub status: u16,
    /// `Content-Type` header value.
    pub content_type: String,
    /// Optional table hint — if the query body contains `"table":"<name>"`,
    /// we store it here to enable per-table invalidation.
    pub table_hint: Option<String>,
    /// When this entry was inserted.
    pub cached_at: Instant,
    /// How long this entry lives.
    pub ttl: Duration,
}

impl CacheEntry {
    pub fn is_expired(&self) -> bool {
        self.cached_at.elapsed() > self.ttl
    }

    /// How old this entry is, in milliseconds — exposed on `X-Cache-Age` header.
    pub fn age_ms(&self) -> u128 {
        self.cached_at.elapsed().as_millis()
    }
}

// ── Cache store ───────────────────────────────────────────────────────────

/// Thread-safe query cache backed by DashMap (lock-free sharded hashmap).
#[derive(Clone)]
pub struct QueryCache {
    inner: Arc<DashMap<QueryCacheKey, CacheEntry>>,
    ttl: Duration,
}

impl QueryCache {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            inner: Arc::new(DashMap::with_capacity(256)),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    /// Look up a request. Returns `None` if absent or expired.
    pub fn get(&self, key: &QueryCacheKey) -> Option<CacheEntry> {
        let entry = self.inner.get(key)?;
        if entry.is_expired() {
            drop(entry);
            self.inner.remove(key);
            return None;
        }
        Some(entry.clone())
    }

    /// Store a successful response. Drops the entry silently if the cache is full.
    pub fn insert(&self, key: QueryCacheKey, entry: CacheEntry) {
        if self.inner.len() >= MAX_ENTRIES {
            // Simple eviction: remove a quarter of expired entries before inserting.
            self.evict_expired();
            if self.inner.len() >= MAX_ENTRIES {
                return; // Still full — skip caching rather than OOM
            }
        }
        self.inner.insert(key, entry);
    }

    /// Build an entry ready for insertion.
    pub fn make_entry(
        &self,
        body: Bytes,
        status: u16,
        content_type: String,
        table_hint: Option<String>,
    ) -> CacheEntry {
        CacheEntry {
            body,
            status,
            content_type,
            table_hint,
            cached_at: Instant::now(),
            ttl: self.ttl,
        }
    }

    /// Invalidate all cached entries for a project, optionally filtered to one table.
    pub fn invalidate(&self, project_id: &str, table: Option<&str>) {
        let keys_to_remove: Vec<QueryCacheKey> = self
            .inner
            .iter()
            .filter(|e| {
                if e.key().project_id != project_id {
                    return false;
                }
                match table {
                    Some(t) => e.value().table_hint.as_deref() == Some(t),
                    None => true, // invalidate whole project
                }
            })
            .map(|e| e.key().clone())
            .collect();

        let count = keys_to_remove.len();
        for k in keys_to_remove {
            self.inner.remove(&k);
        }

        if count > 0 {
            tracing::debug!(
                project_id,
                table = ?table,
                evicted = count,
                "query cache invalidated"
            );
        }
    }

    /// Remove all expired entries. Called by the background eviction task.
    pub fn evict_expired(&self) {
        let expired: Vec<QueryCacheKey> = self
            .inner
            .iter()
            .filter(|e| e.value().is_expired())
            .map(|e| e.key().clone())
            .collect();

        for k in expired {
            self.inner.remove(&k);
        }
    }

    /// Total number of live (non-expired) entries — for metrics/doctor.
    pub fn len(&self) -> usize {
        self.inner.len()
    }
}

// ── Background eviction task ──────────────────────────────────────────────

/// Spawn a task that periodically sweeps expired entries from `cache`.
pub fn start_eviction_task(cache: QueryCache) {
    tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(EVICTION_INTERVAL_SECS));
        loop {
            interval.tick().await;
            let before = cache.inner.len();
            cache.evict_expired();
            let after = cache.inner.len();
            if before > after {
                tracing::debug!(
                    removed = before - after,
                    remaining = after,
                    "query cache: evicted expired entries"
                );
            }
        }
    });
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Try to extract a `"table"` field from the top-level JSON body.
/// Used to tag cache entries for per-table invalidation.
pub fn extract_table_hint(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v.get("table")
        .or_else(|| v.get("from"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
}
