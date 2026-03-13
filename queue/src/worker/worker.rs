//! Worker startup — thin entry point that hands off to the poller.
//!
//! Single responsibility: accept configuration, wire up the worker pipeline,
//! and delegate all polling logic to [`crate::worker::poller::poll`].

use std::sync::Arc;
use sqlx::PgPool;
use job_contract::dispatch::ApiDispatch;

/// Start the background worker pool.
///
/// Blocks forever (runs the poll loop). Call via `tokio::spawn` from `main.rs`.
pub async fn start(
    pool:             PgPool,
    api:              Arc<dyn ApiDispatch>,
    runtime_url:      String,
    service_token:    String,
    concurrency:      usize,
    poll_interval_ms: u64,
) {
    crate::worker::poller::poll(pool, api, runtime_url, service_token, concurrency, poll_interval_ms).await;
}