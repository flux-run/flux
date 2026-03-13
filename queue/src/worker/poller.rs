//! Job poller — the heartbeat of the queue worker pool.
//!
//! ## Responsibilities
//!
//! 1. **Fetch-and-lock**: every `poll_interval_ms` milliseconds, call
//!    [`crate::queue::fetch_jobs::fetch_and_lock_jobs`] to atomically claim up to 20 pending
//!    jobs using `UPDATE … SELECT … FOR UPDATE SKIP LOCKED`. This pattern prevents two
//!    workers from picking the same job, even when multiple queue instances run in parallel.
//!
//! 2. **Semaphore-based concurrency cap**: a `tokio::sync::Semaphore` with `concurrency`
//!    permits ensures at most `concurrency` jobs execute simultaneously. The permit is held
//!    for the lifetime of the job task and dropped automatically when the task completes.
//!
//! 3. **Dispatch to executor**: each fetched job is spawned into `tokio::spawn`; the actual
//!    execution and span emission happen in [`crate::worker::executor::execute`].
//!
//! ## Error handling
//!
//! If the DB fetch fails (e.g. transient connection error), the error is logged and the loop
//! sleeps for `poll_interval_ms` before retrying. A single fetch failure never stops the
//! worker.

use std::sync::Arc;
use std::time::Duration;
use reqwest::Client;
use tokio::sync::Semaphore;
use tokio::time::sleep;
use sqlx::PgPool;
use tracing::error;
use job_contract::dispatch::ApiDispatch;
use crate::queue::fetch_jobs;
use crate::worker::executor;

/// Run the poll loop. Blocks forever — call via `tokio::spawn`.
pub async fn poll(
    pool:             PgPool,
    api:              Arc<dyn ApiDispatch>,
    runtime_url:      String,
    service_token:    String,
    concurrency:      usize,
    poll_interval_ms: u64,
) {
    let client    = Client::new();
    let semaphore = Arc::new(Semaphore::new(concurrency));

    loop {
        match fetch_jobs::fetch_and_lock_jobs(&pool, 20).await {
            Ok(jobs) => {
                for job in jobs {
                    let permit            = semaphore.clone().acquire_owned().await.unwrap();
                    let pool_clone        = pool.clone();
                    let api_clone         = Arc::clone(&api);
                    let runtime_url_clone = runtime_url.clone();
                    let token_clone       = service_token.clone();
                    let client_clone      = client.clone();

                    tokio::spawn(async move {
                        let _permit = permit; // dropped when task completes
                        executor::execute(pool_clone, api_clone, runtime_url_clone, token_clone, client_clone, job).await;
                    });
                }
            }
            Err(e) => error!("Failed to fetch jobs: {}", e),
        }
        sleep(Duration::from_millis(poll_interval_ms)).await;
    }
}