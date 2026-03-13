//! Shared queue application state — injected into every HTTP handler and the worker pool.
//!
//! - `pool` owns job lifecycle DB operations (fetch, lock, update status).
//! - `api`  owns observability writes (spans → `flux.platform_logs`).
//!
//! Separating these two concerns means the queue can be tested with a mock `ApiDispatch`
//! without needing a real `platform_logs` table.

use std::sync::Arc;
use sqlx::PgPool;
use job_contract::dispatch::ApiDispatch;

/// Shared queue application state.
///
/// Cloned into each Axum handler via `axum::extract::State` and forwarded to
/// [`crate::worker::worker::start`] so the worker pool shares the same pool and
/// dispatch instance as the HTTP handlers.
#[derive(Clone)]
pub struct AppState {
    /// Connection pool for all job lifecycle operations.
    pub pool: PgPool,
    /// Control-plane dispatch: writes spans to `flux.platform_logs`.
    pub api:  Arc<dyn ApiDispatch>,
}

impl AppState {
    pub fn new(pool: PgPool, api: Arc<dyn ApiDispatch>) -> Self {
        Self { pool, api }
    }
}
