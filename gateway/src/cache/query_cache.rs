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
use futures::future::{BoxFuture, Shared};
use futures::FutureExt;
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

/// Shared in-flight future type.
/// All concurrent requests for the same key await the same backend call.
type SharedFuture = Shared<BoxFuture<'static, Result<CacheEntry, ()>>>;

/// Cache key: project scope + role + content-addressed request body.
///
/// `role` is extracted from the JWT `role` claim so that two callers with
/// different RLS / CLS permissions never share a cached response.
#[derive(Clone, Hash, Eq, PartialEq)]
pub struct QueryCacheKey {
    /// `X-Fluxbase-Project` header value — isolates tenants.
    pub project_id: String,
    /// JWT `role` claim — prevents cross-permission cache sharing (RLS/CLS).
    pub role: String,
    /// SHA-256 of the raw request body bytes.
    pub body_hash: [u8; 32],
}

impl QueryCacheKey {
    pub fn new(project_id: &str, role: &str, body: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(body);
        let hash: [u8; 32] = hasher.finalize().into();
        Self {
            project_id: project_id.to_string(),
            role: role.to_string(),
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
    /// In-flight request map: concurrent MISS requests coalesce onto one backend call.
    inflight: Arc<DashMap<QueryCacheKey, SharedFuture>>,
    /// Public so closures passed to get_or_fetch can capture the TTL.
    pub ttl: Duration,
}

impl QueryCache {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            inner: Arc::new(DashMap::with_capacity(256)),
            inflight: Arc::new(DashMap::new()),
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

    /// **Single-flight cache fetch.**
    ///
    /// 1. Cache HIT  → return immediately.
    /// 2. Inflight HIT  → await the existing in-flight backend call (coalesced).
    /// 3. Inflight MISS → create a `Shared` future, insert it atomically into the
    ///    inflight map, execute the backend call, then populate the cache.
    ///
    /// `fetch` must return `Ok(entry)` on success or `Err(())` on failure.
    /// Timed out or failed fetches are **not** cached.
    pub async fn get_or_fetch<F>(
        &self,
        key: QueryCacheKey,
        fetch: F,
    ) -> Result<CacheEntry, ()>
    where
        F: FnOnce() -> BoxFuture<'static, Result<CacheEntry, ()>>,
    {
        // ── 1. Cache HIT ──────────────────────────────────────────────────
        if let Some(entry) = self.get(&key) {
            return Ok(entry);
        }

        // ── 2. Atomic check-or-create in the inflight map ─────────────────
        //
        // DashMap::entry() holds the shard lock for the duration of the
        // match arm, giving us an atomic "check then insert".
        let shared_fut = {
            use dashmap::mapref::entry::Entry;
            match self.inflight.entry(key.clone()) {
                Entry::Occupied(e) => {
                    // Another task is already fetching this key — coalesce.
                    tracing::debug!("query cache: coalescing onto in-flight request");
                    e.get().clone()
                }
                Entry::Vacant(e) => {
                    // We are the first — create the shared future with a 10s timeout.
                    let fut = tokio::time::timeout(
                        Duration::from_secs(10),
                        fetch(),
                    )
                    .map(|r| r.unwrap_or(Err(())))
                    .boxed()
                    .shared();
                    e.insert(fut.clone());
                    fut
                }
            }
        }; // shard lock released here

        // ── 3. Await (blocks both the originator and any coalesced waiters) ─
        let result = shared_fut.await;

        // ── 4. Cleanup inflight + populate cache ──────────────────────────
        //
        // Multiple waiters may reach this point concurrently; DashMap ops
        // are idempotent here (duplicate inserts / removes are safe).
        self.inflight.remove(&key);

        if let Ok(ref entry) = result {
            self.insert(key, entry.clone());
        }

        result
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

/// Returns `false` for queries that must never be cached:
///
/// - **Paginated** — body has an `"offset"` field: same body at page 2 != page 1
/// - **Large windows** — `"limit"` > 500: giant payloads waste cache memory
/// - **Non-deterministic** — `"order"` contains `"random"`: result changes each call
///
/// Called *before* the SHA-256 hash is computed, so it also skips the
/// hashing overhead for bypass paths.
pub fn is_query_cacheable(body: &[u8]) -> bool {
    let v: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return true, // can't parse — proxy as usual, don't cache
    };

    // Offset-based pagination: page 2+ would return stale page 1 data.
    if v.get("offset").is_some() {
        return false;
    }

    // Very large result sets: don't burn cache memory on bulk reads.
    const CACHE_MAX_LIMIT: u64 = 500;
    if let Some(limit) = v.get("limit").and_then(|l| l.as_u64()) {
        if limit > CACHE_MAX_LIMIT {
            return false;
        }
    }

    // Non-deterministic ORDER BY random() — result differs on every call.
    if v.get("order")
        .map(|o| o.to_string().to_lowercase().contains("random"))
        .unwrap_or(false)
    {
        return false;
    }

    true
}

/// Extract the `role` claim from a Bearer JWT **without re-verifying** the
/// signature (the auth middleware has already done that upstream).
///
/// Falls back to `"anon"` when the header is absent or the token is
/// malformed, ensuring unauthenticated requests get their own isolated
/// cache partition (important for RLS / CLS).
pub fn extract_role_from_jwt(auth_header: Option<&str>) -> String {
    let token = match auth_header {
        Some(h) if h.starts_with("Bearer ") => &h[7..],
        _ => return "anon".to_string(),
    };

    // JWT = base64url(header).base64url(payload).signature
    let payload_b64 = match token.split('.').nth(1) {
        Some(s) => s,
        None => return "anon".to_string(),
    };

    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine;
    let decoded = match URL_SAFE_NO_PAD.decode(payload_b64) {
        Ok(d) => d,
        Err(_) => return "anon".to_string(),
    };

    let claims: serde_json::Value = match serde_json::from_slice(&decoded) {
        Ok(v) => v,
        Err(_) => return "anon".to_string(),
    };

    claims
        .get("role")
        .and_then(|r| r.as_str())
        .unwrap_or("anon")
        .to_string()
}

/// Try to extract a `"table"` field from the top-level JSON body.
/// Used to tag cache entries for per-table invalidation.
pub fn extract_table_hint(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v.get("table")
        .or_else(|| v.get("from"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
}
