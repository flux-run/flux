use std::sync::Arc;
use sqlx::PgPool;

use crate::cache::CacheManager;
use crate::config::Config;
use crate::file_engine::FileEngine;
use crate::query_guard::QueryGuard;

pub struct AppState {
    pub pool: PgPool,
    pub default_query_limit: i64,
    pub max_query_limit: i64,
    pub runtime_url: String,
    /// Shared HTTP client for hook invocations (connection-pooled).
    pub http_client: reqwest::Client,
    /// All in-process caches and their invalidation logic.
    /// Groups schema cache (L1), plan cache (L2), and policy cache together
    /// so the responsibility for cache management lives in one place (SRP).
    pub cache: CacheManager,
    /// None when FILES_BUCKET is not configured (file uploads disabled).
    pub file_engine: Option<Arc<FileEngine>>,
    /// Complexity ceiling + timeout for all query executions.
    pub query_guard: QueryGuard,
    /// Postgres-level statement timeout (ms) injected via SET LOCAL.
    /// 6× this value is used for replay / internal operations.
    pub statement_timeout_ms: u64,
}

impl AppState {
    pub async fn new(pool: PgPool, cfg: &Config) -> Self {
        let file_engine = if let Some(bucket) = &cfg.s3_bucket {
            Some(Arc::new(
                FileEngine::new(bucket.clone(), cfg.s3_region.clone(), cfg.s3_endpoint.clone()).await,
            ))
        } else {
            tracing::warn!("FILES_BUCKET not set — file engine disabled");
            None
        };

        Self {
            pool,
            default_query_limit: cfg.default_query_limit,
            max_query_limit: cfg.max_query_limit,
            runtime_url: cfg.runtime_url.clone(),
            http_client: reqwest::Client::new(),
            cache: CacheManager::new(),
            file_engine,
            query_guard: QueryGuard::new(cfg.max_query_complexity, cfg.query_timeout_ms, cfg.max_nest_depth),
            statement_timeout_ms: cfg.statement_timeout_ms,
        }
    }
}

