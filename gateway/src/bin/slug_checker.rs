use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = "postgresql://neondb_owner:npg_Y4qPDJWC6oLh@ep-red-water-a1cnxz0z-pooler.ap-southeast-1.aws.neon.tech/neondb?sslmode=require";
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;

    let row: (String, String) = sqlx::query_as("SELECT t.slug, p.slug FROM projects p JOIN tenants t ON p.tenant_id = t.id LIMIT 1")
        .fetch_one(&pool)
        .await?;

    println!("TENANT_SLUG={}", row.0);
    println!("PROJECT_SLUG={}", row.1);

    Ok(())
}
