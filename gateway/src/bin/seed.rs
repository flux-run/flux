use sqlx::postgres::PgPoolOptions;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL is required");
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .after_connect(|conn, _meta| Box::pin(async move {
            sqlx::query("SET search_path = flux, public").execute(conn).await?;
            Ok(())
        }))
        .connect(&database_url)
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
