use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = "postgresql://neondb_owner:npg_Y4qPDJWC6oLh@ep-red-water-a1cnxz0z-pooler.ap-southeast-1.aws.neon.tech/neondb?sslmode=require";
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;

    let row = sqlx::query!("SELECT r.path, r.method FROM routes r JOIN projects p ON r.project_id = p.id WHERE p.slug = $1 LIMIT 1", "new-project-8696")
        .fetch_optional(&pool)
        .await?;

    if let Some(r) = row {
         println!("ROUTE_PATH={}", r.path);
         println!("ROUTE_METHOD={}", r.method);
    } else {
         println!("NO_ROUTE_FOUND");
    }

    Ok(())
}
