use lru::LruCache;
use serde_json::Value;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Cached input/output JSON Schema for a single function.
#[derive(Clone, Debug)]
pub struct FunctionSchema {
    /// JSON Schema for the function's expected input payload.
    /// `None` means no schema is registered — validation is skipped.
    pub input:  Option<Value>,
    /// JSON Schema for the function's expected output.
    /// Stored for introspection; not validated at call time.
    pub output: Option<Value>,
}

/// TTL-based LRU cache mapping `function_id → FunctionSchema`.
///
/// Populated on every cold-path execution when the control-plane
/// `/internal/bundle` response includes `input_schema` / `output_schema`.
/// Invalidated via `POST /internal/cache/invalidate` after re-deploy.
#[derive(Clone)]
pub struct SchemaCache {
    inner: Arc<Mutex<LruCache<String, (FunctionSchema, Instant)>>>,
    ttl:   Duration,
}

impl SchemaCache {
    pub fn new(capacity: usize) -> Self {
        Self::with_ttl(capacity, Duration::from_secs(60))
    }

    pub fn with_ttl(capacity: usize, ttl: Duration) -> Self {
        let cap = NonZeroUsize::new(capacity).unwrap_or(NonZeroUsize::new(200).unwrap());
        Self { inner: Arc::new(Mutex::new(LruCache::new(cap))), ttl }
    }

    pub fn get(&self, function_id: &str) -> Option<FunctionSchema> {
        let mut c = self.inner.lock().unwrap();
        match c.get(function_id) {
            Some((schema, inserted_at)) if inserted_at.elapsed() < self.ttl => {
                Some(schema.clone())
            }
            Some(_) => { c.pop(function_id); None }
            None    => None,
        }
    }

    pub fn insert(&self, function_id: String, schema: FunctionSchema) {
        self.inner.lock().unwrap().put(function_id, (schema, Instant::now()));
    }

    pub fn invalidate(&self, function_id: &str) {
        self.inner.lock().unwrap().pop(function_id);
    }
}
