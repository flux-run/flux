mod config;
mod state;
mod cache;
mod router;
mod routes;
mod services;
mod middleware;
mod clients;

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

    // Initialize snapshot cache
    let snapshot = cache::snapshot::GatewaySnapshot::new(db_pool.clone());
    // Initial fetch to populate caches synchronously before starting server
    if let Err(e) = snapshot.refresh().await {
        tracing::error!("Initial snapshot fetch failed: {:?}", e);
    }
    // Start periodic background refresh
    cache::snapshot::GatewaySnapshot::start_background_refresh(snapshot.clone());

    // Initialize state
    let http_client = reqwest::Client::new();
    let jwks_cache = cache::jwks::JwksCache::new(http_client.clone());
    let queue_client = clients::queue_client::QueueClient::new(
        config.queue_url.clone(),
        http_client.clone(),
    );

    let state = Arc::new(state::GatewayState {
        db_pool,
        http_client,
        runtime_url: config.runtime_url,
        queue_client,
        data_engine_url: config.data_engine_url,
        internal_service_token: config.internal_service_token,
        snapshot,
        jwks_cache,
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
