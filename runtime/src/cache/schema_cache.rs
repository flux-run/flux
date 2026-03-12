use lru::LruCache;
use serde_json::Value;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cached input/output schemas for a single function.
#[derive(Clone, Debug)]
pub struct FunctionSchema {
    /// JSON Schema for the function's expected input payload.
    /// `None` means no schema was provided — validation is skipped.
    pub input: Option<Value>,
    /// JSON Schema for the function's expected output.
    /// Stored for introspection; not validated at call time today.
    pub output: Option<Value>,
}

/// TTL-based LRU cache mapping `function_id → (FunctionSchema, inserted_at)`.
///
/// Populated on every cold-path execution (cache miss on the bundle cache),
/// when the control plane `/internal/bundle` response now includes
/// `input_schema` and `output_schema` fields.
///
/// The cache is intentionally separate from `BundleCache` and `WasmPool`
/// so that schema updates (re-deploy without changing WASM bytes) can be
/// invalidated independently.
#[derive(Clone)]
pub struct SchemaCache {
    inner: Arc<Mutex<LruCache<String, (FunctionSchema, Instant)>>>,
    ttl: Duration,
}

impl SchemaCache {
    /// Create a new cache with the given LRU capacity and the default TTL (60s).
    pub fn new(capacity: usize) -> Self {
        Self::with_ttl(capacity, Duration::from_secs(60))
    }

    pub fn with_ttl(capacity: usize, ttl: Duration) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(200).unwrap());
        Self {
            inner: Arc::new(Mutex::new(LruCache::new(cap))),
            ttl,
        }
    }

    /// Return the cached schema for `function_id` if still within TTL.
    pub fn get(&self, function_id: &str) -> Option<FunctionSchema> {
        let mut c = self.inner.lock().unwrap();
        match c.get(function_id) {
            Some((schema, inserted_at)) if inserted_at.elapsed() < self.ttl => {
                Some(schema.clone())
            }
            Some(_) => {
                c.pop(function_id);
                None
            }
            None => None,
        }
    }

    /// Insert or refresh the schema entry for `function_id`.
    pub fn insert(&self, function_id: String, schema: FunctionSchema) {
        let mut c = self.inner.lock().unwrap();
        c.put(function_id, (schema, Instant::now()));
    }

    /// Evict a stale entry immediately (e.g. after a re-deploy).
    pub fn invalidate(&self, function_id: &str) {
        self.inner.lock().unwrap().pop(function_id);
    }
}
