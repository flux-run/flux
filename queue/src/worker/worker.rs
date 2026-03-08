use sqlx::PgPool;

pub async fn start(pool: PgPool, runtime_url: String, concurrency: usize, poll_interval_ms: u64) {
    crate::worker::poller::poll(pool, runtime_url, concurrency, poll_interval_ms).await;
}