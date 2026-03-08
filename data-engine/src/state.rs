use std::sync::Arc;
use sqlx::PgPool;
use uuid::Uuid;

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
            policy_cache: Arc::new(PolicyCache::new(std::collections::HashMap::new())),
            file_engine,
        }
    }

    /// Evict all cache entries belonging to a tenant+project (called after policy writes).
    pub async fn invalidate_policy_cache(&self, tenant_id: Uuid, project_id: Uuid) {
        let prefix = format!("{}:{}:", tenant_id, project_id);
        let mut guard = self.policy_cache.write().await;
        guard.retain(|k, _| !k.starts_with(&prefix));
    }
}

