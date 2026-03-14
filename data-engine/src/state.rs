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
//! - **`cache`** — Central cache manager owning schema cache (L1 Moka) and plan
//!   cache (L2 Moka). A single `CacheManager` keeps all cache invalidation
//!   logic in one place (SRP).
//!
//! - **`query_guard`** — Enforces maximum query complexity and nesting depth
//!   before compilation starts, preventing expensive query plans from reaching
//!   Postgres.
//!
//! - **`statement_timeout_ms`** — Postgres-level `SET LOCAL statement_timeout`
//!   injected inside each transaction. Postgres cancels the query at the DB engine
//!   level (SQLSTATE 57014) even if the Rust timeout fires first.

use sqlx::PgPool;

use crate::cache::CacheManager;
use crate::config::Config;
use crate::query_guard::QueryGuard;

pub struct AppState {
    pub pool: PgPool,
    pub default_query_limit: i64,
    pub max_query_limit: i64,
    /// All in-process caches and their invalidation logic.
    pub cache: CacheManager,
    /// Complexity ceiling + timeout for all query executions.
    pub query_guard: QueryGuard,
    /// Postgres-level statement timeout (ms) injected via SET LOCAL.
    /// 6× this value is used for replay / internal operations.
    pub statement_timeout_ms: u64,
}

impl AppState {
    pub async fn new(pool: PgPool, cfg: &Config) -> Self {
        Self {
            pool,
            default_query_limit: cfg.default_query_limit,
            max_query_limit: cfg.max_query_limit,
            cache: CacheManager::new(),
            query_guard: QueryGuard::new(cfg.max_query_complexity, cfg.query_timeout_ms, cfg.max_nest_depth),
            statement_timeout_ms: cfg.statement_timeout_ms,
        }
    }
}

