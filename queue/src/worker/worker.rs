use sqlx::PgPool;

pub async fn start(pool: PgPool, runtime_url: String, service_token: String, concurrency: usize, poll_interval_ms: u64) {
    crate::worker::poller::poll(pool, runtime_url, service_token, concurrency, poll_interval_ms).await;
}