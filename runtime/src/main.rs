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
use runtime::dispatch::http_queue::HttpQueueDispatch;
use runtime::engine::executor::PoolDispatchers;
use runtime::engine::pool::IsolatePool;
use runtime::bundle::cache::BundleCache;
use runtime::schema::cache::SchemaCache;
use runtime::execute::handler::execute_handler;
use runtime::execute::invalidate::invalidate_cache_handler;
use runtime::AppState;
use job_contract::dispatch::{ApiDispatch, DataEngineDispatch, QueueDispatch};
use api_contract::routes as R;

/// HTTP implementation of DataEngineDispatch for the standalone runtime binary.
struct HttpDataEngineDispatch {
    client:          reqwest::Client,
    data_engine_url: String,
    service_token:   String,
}

#[async_trait::async_trait]
impl DataEngineDispatch for HttpDataEngineDispatch {
    async fn execute_sql(
        &self,
        sql:        String,
        params:     Vec<serde_json::Value>,
        database:   String,
        request_id: String,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{}/db/query", self.data_engine_url);
        let body = serde_json::json!({ "sql": sql, "params": params, "database": database });
        let resp = self.client
            .post(&url)
            .header("X-Service-Token", &self.service_token)
            .header("x-request-id", &request_id)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("data_engine_unreachable: {}", e))?;
        let status = resp.status();
        let json: serde_json::Value = resp.json().await
            .map_err(|e| format!("data_engine parse error: {}", e))?;
        if !status.is_success() {
            return Err(format!("data_engine HTTP {}: {:?}", status, json));
        }
        Ok(json)
    }
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

    let api_dispatch: Arc<dyn ApiDispatch> = Arc::new(HttpApiDispatch {
        client:  http_client.clone(),
        api_url: settings.api_url.clone(),
        token:   settings.service_token.clone(),
    });

    let queue_dispatch: Arc<dyn QueueDispatch> = Arc::new(HttpQueueDispatch {
        client:    http_client.clone(),
        queue_url: settings.queue_url.clone(),
        token:     settings.service_token.clone(),
    });

    let data_engine_dispatch: Arc<dyn DataEngineDispatch> = Arc::new(HttpDataEngineDispatch {
        client:          http_client.clone(),
        data_engine_url: settings.data_engine_url.clone(),
        service_token:   settings.service_token.clone(),
    });

    let dispatchers = PoolDispatchers {
        api:         Arc::clone(&api_dispatch),
        queue:       Arc::clone(&queue_dispatch),
        data_engine: Arc::clone(&data_engine_dispatch),
        runtime:     Arc::new(std::sync::OnceLock::new()),
    };

    let state = Arc::new(AppState {
        secrets_client:  SecretsClient::new(Arc::clone(&api_dispatch)),
        http_client:     http_client.clone(),
        api:             api_dispatch,
        queue:           queue_dispatch,
        data_engine:     data_engine_dispatch,
        service_token:   settings.service_token.clone(),
        bundle_cache:    BundleCache::new(100),
        schema_cache:    SchemaCache::new(200),
        isolate_pool:    IsolatePool::new(settings.isolate_workers, settings.request_timeout_secs, dispatchers.clone()),
        dispatchers,
    });

    let workers = settings.isolate_workers;
    let app = Router::new()
        .route(R::health::HEALTH.path,         get(health))
        .route(R::health::VERSION.path,         get(move || async move {
            axum::Json(serde_json::json!({
                "service": "runtime",
                "commit":  std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
                "isolate_workers": workers,
            }))
        }))
        .route(R::execution::EXECUTE.path,               post(execute_handler))
        .route(R::internal::CACHE_INVALIDATE.path,        post(invalidate_cache_handler))
        .layer(axum::extract::DefaultBodyLimit::max(1 * 1024 * 1024))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    info!(port, "runtime listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn health() -> axum::Json<serde_json::Value> {
    axum::Json(serde_json::json!({ "status": "ok" }))
}

/// Resolves on SIGTERM (Unix) or Ctrl-C — allows in-flight requests to drain.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        )
        .expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = ctrl_c         => {}
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await;

    tracing::info!("Runtime: shutdown signal received — draining in-flight requests");
}
