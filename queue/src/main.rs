mod config;
mod db;
mod models;
mod services;
mod api;
mod worker;
mod queue;
mod utils;

use std::net::SocketAddr;
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
    ));

    let app = api::routes::routes(pool);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Starting Fluxbase Queue on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
