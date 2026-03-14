//! Central cache manager — owns schema and plan caches and exposes
//! typed invalidation helpers.
//!
//! ## What is cached
//!
//! | Cache | Backend | Key | Value | Why |
//! |---|---|---|---|---|
//! | `schema_cache` | Moka (L1) | `"{schema}:{table}"` | column metadata + relationships | Avoid `information_schema` round-trips on every query |
//! | `plan_cache`   | Moka (L2) | `PlanKey { schema, table, operation, … }` | compiled SQL template | Avoid re-running the compiler on repeated identical queries |
//!
//! ## Invalidation
//!
//! All invalidation is explicit (no TTL), driven by API events:
//! - **DDL change** (column add/drop) → `invalidate_table`
//! - **Relationship change** → `invalidate_schema`
//! - **Project-wide reset** → `invalidate_all`

use crate::cache::{
    build_plan_cache, build_schema_cache, schema_key, schema_prefix, PlanCache, SchemaCache,
};

pub struct CacheManager {
    pub schema_cache: SchemaCache,
    pub plan_cache: PlanCache,
}

impl CacheManager {
    pub fn new() -> Self {
        Self {
            schema_cache: build_schema_cache(),
            plan_cache: build_plan_cache(),
        }
    }

    // ── Invalidation helpers ──────────────────────────────────────────────────

    /// Evict schema + plan cache entries for one specific table.
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
    pub fn invalidate_all(&self) {
        self.schema_cache.invalidate_all();
        self.plan_cache.invalidate_all();
    }
}

impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}
