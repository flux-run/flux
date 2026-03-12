use sqlx::{postgres::PgPoolOptions, PgPool};

pub async fn init_pool(database_url: &str) -> Result<PgPool, sqlx::Error> {
    PgPoolOptions::new()
        .max_connections(10)
        .after_connect(|conn, _meta| Box::pin(async move {
            sqlx::query("SET search_path = flux, public").execute(conn).await?;
            Ok(())
        }))
        .connect(database_url)
        .await
}

pub async fn migrate(pool: &PgPool) -> Result<(), sqlx::migrate::MigrateError> {
    sqlx::migrate!("./migrations").run(pool).await
}