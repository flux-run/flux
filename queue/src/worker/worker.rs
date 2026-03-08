use sqlx::PgPool;

pub async fn start(pool: PgPool, runtime_url: String) {
    crate::worker::poller::poll(pool, runtime_url).await;
}