mod api;
mod config;
mod engine;
mod secrets;
mod cache;

use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

use api::routes::{execute_handler, health_check, AppState};
use config::settings::Settings;
use secrets::secrets_client::SecretsClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::load();
    let port = settings.port;

    let secrets_client = SecretsClient::new(settings.clone());
    
    let state = Arc::new(AppState {
        secrets_client,
        control_plane_url: settings.control_plane_url.clone(),
        service_token: settings.service_token.clone(),
        bundle_cache: cache::bundle_cache::BundleCache::new(100),
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/execute", post(execute_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Starting Runtime execution server on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
