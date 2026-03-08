use std::collections::HashMap;
use std::sync::Arc;
use sqlx::PgPool;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::cache::{PlanCache, SchemaCache, schema_key, tenant_prefix};
use crate::config::Config;
use crate::file_engine::FileEngine;
use crate::policy::PolicyCache;

pub struct AppState {
    pub pool: PgPool,
    pub default_query_limit: i64,
    pub max_query_limit: i64,
    pub runtime_url: String,
    /// Shared HTTP client for hook invocations (connection-pooled).
    pub http_client: reqwest::Client,
    pub policy_cache: Arc<PolicyCache>,
    /// None when S3_BUCKET is not configured (file uploads disabled).
    pub file_engine: Option<Arc<FileEngine>>,
    /// Layer-1 cache: schema metadata (col_meta + relationships) per table.
    pub schema_cache: Arc<SchemaCache>,
    /// Layer-2 cache: compiled SELECT SQL templates keyed by request shape.
    pub plan_cache: Arc<PlanCache>,
}

impl AppState {
    pub async fn new(pool: PgPool, cfg: &Config) -> Self {
        let file_engine = if let Some(bucket) = &cfg.s3_bucket {
            Some(Arc::new(
                FileEngine::new(bucket.clone(), cfg.s3_region.clone(), cfg.s3_endpoint.clone()).await,
            ))
        } else {
            tracing::warn!("S3_BUCKET not set — file engine disabled");
            None
        };

        Self {
            pool,
            default_query_limit: cfg.default_query_limit,
            max_query_limit: cfg.max_query_limit,
            runtime_url: cfg.runtime_url.clone(),
            http_client: reqwest::Client::new(),
            policy_cache: Arc::new(PolicyCache::new(HashMap::new())),
            file_engine,
            schema_cache: Arc::new(RwLock::new(HashMap::new())),
            plan_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Evict all cache entries belonging to a tenant+project (called after policy writes).
    pub async fn invalidate_policy_cache(&self, tenant_id: Uuid, project_id: Uuid) {
        let prefix = format!("{}:{}:", tenant_id, project_id);
        let mut guard = self.policy_cache.write().await;
        guard.retain(|k, _| !k.starts_with(&prefix));
    }

    /// Precise invalidation: evict one table's schema + plan cache entries.
    /// Called after `CREATE TABLE`, `ALTER TABLE`, or `DROP TABLE`.
    pub async fn invalidate_table(&self, tenant_id: Uuid, project_id: Uuid, schema: &str, table: &str) {
        let key = schema_key(tenant_id, project_id, schema, table);
        self.schema_cache.write().await.remove(&key);
        self.plan_cache.write().await.retain(|k, _| {
            !(k.tenant_id == tenant_id
                && k.project_id == project_id
                && k.schema == schema
                && k.table == table)
        });
    }

    /// Broad invalidation: evict all schema + plan cache entries for a tenant+project.
    /// Called after relationship or hook changes where the table name is not
    /// readily available without an extra DB query.
    pub async fn invalidate_tenant_schema(&self, tenant_id: Uuid, project_id: Uuid) {
        let prefix = tenant_prefix(tenant_id, project_id);
        self.schema_cache.write().await.retain(|k, _| !k.starts_with(&prefix));
        self.plan_cache.write().await.retain(|k, _| {
            !(k.tenant_id == tenant_id && k.project_id == project_id)
        });
    }
}

