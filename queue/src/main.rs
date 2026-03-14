//! Queue entry point — thin startup wrapper.
//!
//! All module declarations live in lib.rs.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::info;

use fluxbase_queue::{api, config, db, dispatch, state, worker};

/// Minimal HTTP implementation of [`RuntimeDispatch`] for the queue standalone binary.
struct HttpRuntimeDispatch {
    client:        reqwest::Client,
    runtime_url:   String,
    service_token: String,
}

#[async_trait::async_trait]
impl job_contract::dispatch::RuntimeDispatch for HttpRuntimeDispatch {
    async fn execute(
        &self,
        req: job_contract::dispatch::ExecuteRequest,
    ) -> Result<job_contract::dispatch::ExecuteResponse, String> {
        let url = format!("{}/execute", self.runtime_url);
        let body = serde_json::json!({
            "function_id": req.function_id,
            "payload":     req.payload,
        });
        let resp = self.client
            .post(&url)
            .header("X-Service-Token", &self.service_token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("runtime_unreachable: {}", e))?;
        let status = resp.status().as_u16();
        let raw    = resp.text().await.unwrap_or_default();
        let body: serde_json::Value = serde_json::from_str(&raw).unwrap_or_else(|_| {
            serde_json::json!({ "error": "runtime_response_parse_error", "raw": raw })
        });
        Ok(job_contract::dispatch::ExecuteResponse { body, status, duration_ms: 0 })
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::config::init();

    let config = config::config::load();
    let pool = db::connection::init_pool(&config.database_url, config.worker_concurrency).await?;

    let api_dispatch: Arc<dyn job_contract::dispatch::ApiDispatch> =
        Arc::new(dispatch::HttpApiDispatch {
            client:  reqwest::Client::new(),
            api_url: config.api_url.clone(),
            token:   config.service_token.clone(),
        });

    let runtime_dispatch: Arc<dyn job_contract::dispatch::RuntimeDispatch> =
        Arc::new(HttpRuntimeDispatch {
            client:        reqwest::Client::new(),
            runtime_url:   config.runtime_url.clone(),
            service_token: config.service_token.clone(),
        });

    // Shutdown channel: send () to signal all background tasks to stop.
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    tokio::spawn(worker::worker::start(
        pool.clone(),
        Arc::clone(&api_dispatch),
        Arc::clone(&runtime_dispatch),
        config.service_token.clone(),
        config.worker_concurrency,
        config.poll_interval_ms,
        shutdown_rx.clone(),
    ));

    tokio::spawn(worker::timeout_recovery::run(
        pool.clone(),
        config.job_timeout_check_interval_ms,
        shutdown_rx.clone(),
    ));

    let app_state = Arc::new(state::AppState::new(pool, api_dispatch));
    let app = api::routes::routes(app_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Starting Fluxbase Queue on {}", addr);
    let listener = TcpListener::bind(addr).await?;

    // Serve until SIGTERM or Ctrl-C.
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            info!("Shutdown signal received — stopping workers");
            // Notify background tasks to stop.
            let _ = shutdown_tx.send(());
        })
        .await?;

    info!("Queue shutdown complete");
    Ok(())
}

/// Resolves on SIGTERM (Unix) or Ctrl-C (all platforms).
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
            _ = ctrl_c            => {}
            _ = sigterm.recv()    => {}
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await;
}
