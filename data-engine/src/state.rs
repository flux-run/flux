//! Shared application state for the data-engine service.
//!
//! ## Fields
//!
//! - **`pool`** — Postgres connection pool. All query execution and mutation
//!   logging share this pool. The data-engine is the sole writer to project
//!   schemas; having one pool enforces serialisation.
//!
//! - **`default_query_limit`** — Rows returned per query when the caller omits
//!   a `limit` field. Protects against unbounded SELECTs on large tables.
//!
//! - **`max_query_limit`** — Hard ceiling clamped by the compiler even when the
//!   caller explicitly requests more rows. Prevents individual queries from
//!   saturating the connection pool.
//!
//! - **`runtime_url`** — Base URL of the Runtime service. Used by `HookEngine`
//!   to POST before/after hook payloads to the runtime (`POST /execute`).
//!
//! - **`http_client`** — Shared reqwest client for hook invocations. Connection
//!   pool is reused across requests to avoid per-hook TCP handshake overhead.
//!
//! - **`cache`** — Central cache manager owning schema cache (L1 Moka), plan
//!   cache (L2 Moka), and policy cache (RwLock HashMap). A single `CacheManager`
//!   keeps all cache invalidation logic in one place (SRP).
//!
//! - **`file_engine`** — Optional file upload/download engine backed by S3/MinIO.
//!   `None` when `FILES_BUCKET` is not configured (file operations disabled).
//!
//! - **`query_guard`** — Enforces maximum query complexity and nesting depth
//!   before compilation starts, preventing expensive query plans from reaching
//!   Postgres.
//!
//! - **`statement_timeout_ms`** — Postgres-level `SET LOCAL statement_timeout`
//!   injected inside each transaction. Postgres cancels the query at the DB engine
//!   level (SQLSTATE 57014) even if the Rust timeout fires first.

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

