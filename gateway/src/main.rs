mod config;
mod state;
mod router;
mod routes;
mod services;
mod middleware;

use axum::Router;
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Load configuration
    dotenvy::dotenv().ok();
    let config = config::Config::load();

    // Setup database pool
    let db_pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(&config.database_url)
        .await?;

    info!("Gateway connected to database");

    // Initialize state
    let state = Arc::new(state::GatewayState {
        db_pool,
        http_client: reqwest::Client::new(),
        runtime_url: config.runtime_url,
        internal_service_token: config.internal_service_token,
    });

    // Build router
    let app = router::create_router(state);

    // Start server
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Starting Fluxbase Gateway on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
