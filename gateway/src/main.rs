//! Gateway entry point.
//!
//! Responsibilities (startup only):
//!   1. Load config from environment / .env
//!   2. Connect to database, warm the route snapshot
//!   3. Wire shared state
//!   4. Build the Axum router and start listening
mod config;
mod state;
mod router;
mod snapshot;
mod auth;
mod rate_limit;
mod trace;
mod forward;
mod handlers;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpListener;
use tracing::info;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    // Load .env if present — silently ignored in production where env vars are
    // injected directly by the container runtime.
    dotenvy::dotenv().ok();

    let config = config::Config::load();

    // Database pool.
    let db_pool = PgPoolOptions::new()
        .max_connections(20)
        .after_connect(|conn, _meta| Box::pin(async move {
            sqlx::query("SET search_path = flux, public").execute(conn).await?;
            Ok(())
        }))
        .connect(&config.database_url)
        .await?;

    info!("Gateway connected to database");

    // Route snapshot — warm before accepting traffic so /readiness returns 200.
    let snapshot = snapshot::GatewaySnapshot::new(
        db_pool.clone(),
        config.database_url.clone(),
    );
    // Warm the snapshot before accepting traffic.
    if let Err(e) = snapshot.refresh().await {
        tracing::warn!("Initial snapshot fetch failed (will retry on first NOTIFY): {:?}", e);
    }
    // LISTEN/NOTIFY keeps the snapshot current — no polling needed.
    snapshot::GatewaySnapshot::start_notify_listener(snapshot.clone());

    // HTTP client with a timeout matching RUNTIME_TIMEOUT_SECS.
    let http_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(config.runtime_timeout_secs))
        .build()?;

    let jwks_cache = auth::JwksCache::new(http_client.clone());

    let state = Arc::new(state::GatewayState {
        db_pool,
        http_client,
        runtime_url:            config.runtime_url,
        internal_service_token: config.internal_service_token,
        snapshot,
        jwks_cache,
        max_request_size_bytes: config.max_request_size_bytes,
        rate_limit_per_sec:     config.rate_limit_per_sec,
        local_mode:             config.local_mode,
    });

    let app  = router::create_router(state);
    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Flux Gateway listening on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
