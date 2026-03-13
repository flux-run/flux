//! Central cache manager — owns all three in-process caches and exposes
//! typed invalidation helpers.
//!
//! ## What is cached
//!
//! | Cache | Backend | Key | Value | Why |
//! |---|---|---|---|---|
//! | `schema_cache` | Moka (L1) | `"{schema}:{table}"` | column metadata + relationships | Avoid `information_schema` round-trips on every query |
//! | `plan_cache`   | Moka (L2) | `PlanKey { schema, table, operation, … }` | compiled SQL template | Avoid re-running the compiler on repeated identical queries |
//! | `policy_cache` | `RwLock<HashMap>` | `"{table}:{role}:{operation}"` | `PolicyResult` | Avoid `fluxbase_internal.policies` lookup on every query |
//!
//! ## Invalidation
//!
//! All invalidation is explicit (no TTL), driven by API events:
//! - **DDL change** (column add/drop) → `invalidate_table`
//! - **Hook / relationship change** → `invalidate_schema`
//! - **Policy add/delete** → `invalidate_policy`
//! - **Project-wide reset** → `invalidate_all`
//!
//! Centralising these helpers in `CacheManager` (rather than scattering `cache.invalidate()`
//! calls across API handlers) is the SRP win: the cache knows how to invalidate itself.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::cache::{
    build_plan_cache, build_schema_cache, schema_key, schema_prefix, PlanCache, SchemaCache,
};
use crate::policy::PolicyCache;

pub struct CacheManager {
    pub schema_cache: SchemaCache,
    pub plan_cache: PlanCache,
    /// Row-level policy evaluation results.
    /// Wrapped in `Arc` so `PolicyEngine::evaluate_cached` can borrow it
    /// without returning a temporary reference.
    pub policy_cache: Arc<PolicyCache>,
}

impl CacheManager {
    pub fn new() -> Self {
        Self {
            schema_cache: build_schema_cache(),
            plan_cache: build_plan_cache(),
            policy_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    // ── Invalidation helpers ──────────────────────────────────────────────────

    /// Evict all policy-cache entries.
    /// Called whenever a policy row is created or deleted.
    pub async fn invalidate_policy(&self) {
        let mut guard = self.policy_cache.write().await;
        guard.clear();
    }

    /// Evict schema + plan cache entries for one specific table.
    /// Called after DDL changes to a single table (CREATE TABLE, DROP TABLE,
    /// or column metadata updates).
    pub fn invalidate_table(&self, schema: &str, table: &str) {
        let key = schema_key(schema, table);
        self.schema_cache.invalidate(&key);
        let s = schema.to_owned();
        let t = table.to_owned();
        self.plan_cache
            .invalidate_entries_if(move |k, _| k.schema == s && k.table == t)
            .ok();
    }

    /// Evict all schema + plan cache entries for a schema.
    /// Called after hook / relationship / policy changes that affect the
    /// schema fingerprint without targeting a single table.
    pub fn invalidate_schema(&self, schema: &str) {
        let prefix = schema_prefix(schema);
        self.schema_cache
            .invalidate_entries_if(move |k: &String, _| k.starts_with(&prefix))
            .ok();
        let schema_owned = schema.to_owned();
        self.plan_cache
            .invalidate_entries_if(move |k, _| k.schema == schema_owned)
            .ok();
    }

    /// Evict everything in the schema + plan caches.
    /// Called after project-wide config changes (hooks, relationships).
    pub fn invalidate_all(&self) {
        self.schema_cache.invalidate_all();
        self.plan_cache.invalidate_all();
    }
}

// Convenience so callers can write `CacheManager::default()` in tests.
impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}
