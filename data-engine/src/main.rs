mod api;
mod cache;
mod compiler;
mod config;
mod cron;
mod db;
mod engine;
mod events;
mod executor;
mod file_engine;
mod hooks;
mod policy;
mod query_guard;
mod router;
mod state;
mod telemetry;
mod transform;
mod workflow;

use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::init();    // loads .env
    telemetry::init(); // sets up tracing subscriber
    let cfg = config::load();

    tracing::info!("connecting to database...");
    // init_pool_with_identity_log logs the system_identifier + db_name at startup.
    // For user-provided (BYODB) pools call verify_db_identity() after pool construction
    // to enforce the expected identity stored in project_databases.
    let pool = db::connection::init_pool_with_identity_log(&cfg.database_url, "platform").await;

    let app_state = Arc::new(state::AppState::new(pool.clone(), &cfg).await);

    // Spawn background workers — each shares the pool but runs independently.
    let worker_pool = Arc::new(pool);
    let worker_http = Arc::new(app_state.http_client.clone());
    let worker_runtime_url = cfg.runtime_url.clone();

    // Events delivery worker
    let ev_pool = Arc::clone(&worker_pool);
    let ev_http = Arc::clone(&worker_http);
    let ev_url = worker_runtime_url.clone();
    tokio::spawn(async move {
        events::worker::run(ev_pool, ev_http, ev_url).await;
    });

    // Workflow step-advancement worker
    let wf_pool = Arc::clone(&worker_pool);
    let wf_http = Arc::clone(&worker_http);
    let wf_url = worker_runtime_url.clone();
    tokio::spawn(async move {
        workflow::engine::run(wf_pool, wf_http, wf_url).await;
    });

    // Cron scheduler worker
    let cron_pool = Arc::clone(&worker_pool);
    let cron_http = Arc::clone(&worker_http);
    let cron_url = worker_runtime_url.clone();
    tokio::spawn(async move {
        cron::worker::run(cron_pool, cron_http, cron_url).await;
    });

    let app = api::routes::build(app_state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("data-engine listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
