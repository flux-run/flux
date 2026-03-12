use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = "postgresql://neondb_owner:npg_Y4qPDJWC6oLh@ep-red-water-a1cnxz0z-pooler.ap-southeast-1.aws.neon.tech/neondb?sslmode=require";
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|conn, _meta| Box::pin(async move {
            sqlx::query("SET search_path = flux, public").execute(conn).await?;
            Ok(())
        }))
        .connect(database_url)
        .await?;

    println!("Applying migration...");
    
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS routes ( \
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(), \
            project_id UUID NOT NULL, \
            path TEXT NOT NULL, \
            method TEXT NOT NULL, \
            function_id UUID NOT NULL, \
            auth_type TEXT NOT NULL DEFAULT 'none', \
            cors_enabled BOOLEAN NOT NULL DEFAULT false, \
            rate_limit INTEGER, \
            created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP, \
            UNIQUE(path, method) \
        );"
    )
    .execute(&pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_routes_path_method ON routes(path, method);")
        .execute(&pool)
        .await?;

    println!("✅ Migration applied!");
    Ok(())
}
