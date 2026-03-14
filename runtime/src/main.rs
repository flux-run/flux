//! Runtime entry point — thin startup wrapper.
//!
//! All module declarations live in lib.rs; this file only owns the
//! tokio::main startup task.

use std::sync::Arc;
use axum::{routing::{get, post}, Router};
use tokio::net::TcpListener;
use tracing::info;

use runtime::config::settings::Settings;
use runtime::secrets::client::SecretsClient;
use runtime::dispatch::http_api::HttpApiDispatch;
use runtime::engine::pool::IsolatePool;
use runtime::engine::wasm_pool::WasmPool;
use runtime::bundle::cache::BundleCache;
use runtime::schema::cache::SchemaCache;
use runtime::execute::handler::execute_handler;
use runtime::execute::invalidate::invalidate_cache_handler;
use runtime::AppState;
use job_contract::dispatch::ApiDispatch;

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

    let api_dispatch: Arc<dyn ApiDispatch> = Arc::new(HttpApiDispatch {
        client:  http_client.clone(),
        api_url: settings.api_url.clone(),
        token:   settings.service_token.clone(),
    });

    let state = Arc::new(AppState {
        secrets_client: SecretsClient::new(Arc::clone(&api_dispatch)),
        http_client:    http_client.clone(),
        api:            api_dispatch,
        api_url:        settings.api_url.clone(),
        queue_url:      settings.queue_url.clone(),
        service_token:  settings.service_token.clone(),
        bundle_cache:   BundleCache::new(100),
        schema_cache:   SchemaCache::new(200),
        isolate_pool:   IsolatePool::new(settings.isolate_workers, settings.request_timeout_secs),
        wasm_pool:      WasmPool::new(
            std::thread::available_parallelism().map(|n| (n.get() * 2).clamp(2, 16)).unwrap_or(4),
            settings.wasm_fuel_limit,
            settings.request_timeout_secs,
        ),
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
