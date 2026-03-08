mod config;
mod db;
mod models;
mod services;
mod api;
mod worker;
mod queue;
mod utils;
mod state;

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::config::init();

    let config = config::config::load();
    let pool = db::connection::init_pool(&config.database_url).await?;
    db::connection::migrate(&pool).await?;

    tokio::spawn(worker::worker::start(
        pool.clone(),
        config.runtime_url.clone(),
        config.worker_concurrency,
        config.poll_interval_ms,
    ));

    let app_state = Arc::new(state::AppState::new(pool));
    let app = api::routes::routes(app_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Starting Fluxbase Queue on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
