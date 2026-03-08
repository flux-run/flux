use std::sync::Arc;
use sqlx::PgPool;
use uuid::Uuid;

use crate::config::Config;
use crate::policy::PolicyCache;

pub struct AppState {
    pub pool: PgPool,
    pub default_query_limit: i64,
    pub policy_cache: Arc<PolicyCache>,
}

impl AppState {
    pub fn new(pool: PgPool, cfg: &Config) -> Self {
        Self {
            pool,
            default_query_limit: cfg.default_query_limit,
            policy_cache: Arc::new(PolicyCache::new(std::collections::HashMap::new())),
        }
    }

    /// Evict all cache entries belonging to a tenant+project (called after policy writes).
    pub async fn invalidate_policy_cache(&self, tenant_id: Uuid, project_id: Uuid) {
        let prefix = format!("{}:{}:", tenant_id, project_id);
        let mut guard = self.policy_cache.write().await;
        guard.retain(|k, _| !k.starts_with(&prefix));
    }
}

