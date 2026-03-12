mod config;
mod execute;
mod trace;
mod schema;
mod bundle;
mod secrets;
mod engine;
mod agent;

use std::sync::Arc;
use axum::{routing::{get, post}, Router};
use tokio::net::TcpListener;
use tracing::info;

use config::settings::Settings;
use secrets::client::SecretsClient;
use engine::pool::IsolatePool;
use engine::wasm_pool::WasmPool;
use bundle::cache::BundleCache;
use schema::cache::SchemaCache;
use execute::handler::execute_handler;
use execute::invalidate::invalidate_cache_handler;

// ── AppState ──────────────────────────────────────────────────────────────────

pub struct AppState {
    pub secrets_client: SecretsClient,
    pub http_client:    reqwest::Client,
    pub api_url:        String,
    pub service_token:  String,
    pub bundle_cache:   BundleCache,
    pub schema_cache:   SchemaCache,
    pub isolate_pool:   IsolatePool,
    pub wasm_pool:      WasmPool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let settings = Settings::load();
    let port     = settings.port;

    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .tcp_keepalive(std::time::Duration::from_secs(30))
        .build()
        .expect("failed to build HTTP client");

    let state = Arc::new(AppState {
        secrets_client: SecretsClient::new(settings.clone(), http_client.clone()),
        http_client:    http_client.clone(),
        api_url:        settings.api_url.clone(),
        service_token:  settings.service_token.clone(),
        bundle_cache:   BundleCache::new(100),
        schema_cache:   SchemaCache::new(200),
        isolate_pool:   IsolatePool::new(settings.isolate_workers),
        wasm_pool:      WasmPool::default_sized(),
    });

    let workers = settings.isolate_workers;
    let app = Router::new()
        .route("/health",  get(health))
        .route("/version", get(move || async move {
            axum::Json(serde_json::json!({
                "service": "runtime",
                "commit":  std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
                "isolate_workers": workers,
            }))
        }))
        .route("/execute",                   post(execute_handler))
        .route("/internal/cache/invalidate", post(invalidate_cache_handler))
        .layer(axum::extract::DefaultBodyLimit::max(1 * 1024 * 1024))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    info!(port, "runtime listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}
