//! Worker startup — thin entry point that hands off to the poller.
//!
//! Single responsibility: accept configuration, wire up the worker pipeline,
//! and delegate all polling logic to [`crate::worker::poller::poll`].

use std::sync::Arc;
use sqlx::PgPool;
use tokio::sync::watch;
use job_contract::dispatch::ApiDispatch;

/// Start the background worker pool.
///
/// Runs the poll loop until `shutdown_rx` receives a value. Call via `tokio::spawn`.
pub async fn start(
    pool:             PgPool,
    api:              Arc<dyn ApiDispatch>,
    runtime_url:      String,
    service_token:    String,
    concurrency:      usize,
    poll_interval_ms: u64,
    shutdown_rx:      watch::Receiver<()>,
) {
    crate::worker::poller::poll(pool, api, runtime_url, service_token, concurrency, poll_interval_ms, shutdown_rx).await;
}