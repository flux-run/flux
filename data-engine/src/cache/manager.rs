//! Central cache manager — owns all three in-process caches and exposes
//! typed invalidation helpers.
//!
//! **Why a CacheManager?**
//!
//! Previously `AppState` held the cache fields *and* contained three
//! invalidation methods that mixed cache-management logic into the state
//! struct (SRP violation). `CacheManager` owns:
//!
//! * `schema_cache` — Moka: `(tenant, project, schema, table)` → col_meta + rels
//! * `plan_cache`   — Moka: `PlanKey` → compiled SQL template
//! * `policy_cache` — `RwLock<HashMap>`: policy evaluation results
//!
//! Callers that previously wrote `state.invalidate_tenant_schema(…)` now write
//! `state.cache.invalidate_tenant(…)`, keeping the concern inside this module.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;
use uuid::Uuid;

use crate::cache::{
    build_plan_cache, build_schema_cache, schema_key, tenant_prefix, PlanCache, SchemaCache,
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

    /// Evict all policy-cache entries for a tenant+project.
    /// Called whenever a policy row is created or deleted.
    pub async fn invalidate_policy(&self, tenant_id: Uuid, project_id: Uuid) {
        let prefix = format!("{}:{}:", tenant_id, project_id);
        let mut guard = self.policy_cache.write().await;
        guard.retain(|k, _| !k.starts_with(&prefix));
    }

    /// Evict schema + plan cache entries for one specific table.
    /// Called after DDL changes to a single table (CREATE TABLE, DROP TABLE,
    /// or column metadata updates).
    pub fn invalidate_table(
        &self,
        tenant_id: Uuid,
        project_id: Uuid,
        schema: &str,
        table: &str,
    ) {
        let key = schema_key(tenant_id, project_id, schema, table);
        self.schema_cache.invalidate(&key);
        let s = schema.to_owned();
        let t = table.to_owned();
        self.plan_cache
            .invalidate_entries_if(move |k, _| {
                k.tenant_id == tenant_id
                    && k.project_id == project_id
                    && k.schema == s
                    && k.table == t
            })
            .ok();
    }

    /// Evict all schema + plan cache entries for a tenant+project.
    /// Called after hook / relationship / policy changes that affect the
    /// schema fingerprint without targeting a single table.
    pub fn invalidate_tenant(&self, tenant_id: Uuid, project_id: Uuid) {
        let prefix = tenant_prefix(tenant_id, project_id);
        self.schema_cache
            .invalidate_entries_if(move |k: &String, _| k.starts_with(&prefix))
            .ok();
        self.plan_cache
            .invalidate_entries_if(move |k, _| {
                k.tenant_id == tenant_id && k.project_id == project_id
            })
            .ok();
    }
}

// Convenience so callers can write `CacheManager::default()` in tests.
impl Default for CacheManager {
    fn default() -> Self {
        Self::new()
    }
}
