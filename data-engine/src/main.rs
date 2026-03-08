mod api;
mod compiler;
mod config;
mod db;
mod engine;
mod events;
mod executor;
mod file_engine;
mod hooks;
mod policy;
mod router;
mod state;
mod transform;

use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::init();
    let cfg = config::load();

    tracing::info!("connecting to database...");
    let pool = db::connection::init_pool(&cfg.database_url).await;

    tracing::info!("running migrations...");
    sqlx::migrate!("./migrations").run(&pool).await?;

    let app_state = Arc::new(state::AppState::new(pool, &cfg).await);
    let app = api::routes::build(app_state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("data-engine listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
