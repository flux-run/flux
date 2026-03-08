use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let database_url = "postgresql://neondb_owner:npg_Y4qPDJWC6oLh@ep-red-water-a1cnxz0z-pooler.ap-southeast-1.aws.neon.tech/neondb?sslmode=require";
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(database_url)
        .await?;

    // 1. Get function and project info for 'hello'
    let row: (Uuid, Uuid) = sqlx::query_as("SELECT id, project_id FROM functions WHERE name = 'hello' LIMIT 1")
        .fetch_one(&pool)
        .await?;
    
    let function_id = row.0;
    let project_id = row.1;

    println!("Found function {} for project {}", function_id, project_id);

    // 2. Insert route
    sqlx::query(
        "INSERT INTO routes (project_id, path, method, function_id, auth_type, cors_enabled, rate_limit) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (path, method) DO UPDATE SET function_id = EXCLUDED.function_id"
    )
    .bind(project_id)
    .bind("/hello")
    .bind("POST")
    .bind(function_id)
    .bind("none")
    .bind(false)
    .bind(100) // 100 rpm
    .execute(&pool)
    .await?;

    println!("✅ Successfully seeded route POST /hello -> {}", function_id);
    Ok(())
}
