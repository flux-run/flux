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

use api::routes::{execute_handler, health_check, invalidate_cache_handler, AppState};
use config::settings::Settings;
use secrets::secrets_client::SecretsClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::load();
    let port = settings.port;

    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .tcp_keepalive(std::time::Duration::from_secs(30))
        .connection_verbose(false)
        .build()
        .expect("failed to build HTTP client");

    let secrets_client = SecretsClient::new(settings.clone(), http_client.clone());
    
    let state = Arc::new(AppState {
        secrets_client,
        http_client,
        control_plane_url: settings.control_plane_url.clone(),
        service_token: settings.service_token.clone(),
        bundle_cache: cache::bundle_cache::BundleCache::new(100),
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/version", get(|| async {
            axum::Json(serde_json::json!({
                "service": "runtime",
                "commit": std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
                "build_time": std::env::var("BUILD_TIME").unwrap_or_else(|_| "unknown".to_string())
            }))
        }))
        .route("/execute", post(execute_handler))
        .route("/internal/cache/invalidate", post(invalidate_cache_handler))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Starting Runtime execution server on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    
    axum::serve(listener, app).await?;
    
    Ok(())
}
