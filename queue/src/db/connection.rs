use sqlx::{postgres::PgPoolOptions, PgPool};

/// Create a Postgres pool sized for the given worker concurrency.
///
/// Pool size = `worker_concurrency + 5` so there is always headroom for
/// API/admin requests even when all worker slots are occupied. Without this
/// a pool of 10 against 50 concurrent workers causes connection starvation.
pub async fn init_pool(database_url: &str, worker_concurrency: usize) -> Result<PgPool, sqlx::Error> {
    let max_connections = (worker_concurrency + 5).max(10) as u32;
    PgPoolOptions::new()
        .max_connections(max_connections)
        .after_connect(|conn, _meta| Box::pin(async move {
            sqlx::query("SET search_path = flux, public").execute(conn).await?;
            Ok(())
        }))
        .connect(database_url)
        .await
}