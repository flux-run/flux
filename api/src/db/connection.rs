use sqlx::{postgres::PgPoolOptions, PgPool};
use std::env;
use tracing::info;

pub async fn init_pool() -> Result<PgPool, sqlx::Error> {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    info!("Connecting to database...");

    let pool = PgPoolOptions::new()
        .max_connections(5)
        .after_connect(|conn, _meta| Box::pin(async move {
            // Keep Fluxbase-internal tables in the `flux` schema.
            // User application tables live in `public`.
            sqlx::query("SET search_path = flux, public").execute(conn).await?;
            Ok(())
        }))
        .connect(&database_url)
        .await?;

    info!("Database connection established");

    Ok(pool)
}
