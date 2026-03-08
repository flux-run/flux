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

    let app_state = Arc::new(state::AppState::new(pool.clone(), &cfg).await);

    // Spawn the event worker as a background task — it shares the pool but
    // runs independently of the HTTP server.
    let worker_pool = Arc::new(pool);
    let worker_http = Arc::new(app_state.http_client.clone());
    let worker_runtime_url = cfg.runtime_url.clone();
    tokio::spawn(async move {
        events::worker::run(worker_pool, worker_http, worker_runtime_url).await;
    });

    let app = api::routes::build(app_state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("data-engine listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
