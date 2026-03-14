//! Queue entry point — thin startup wrapper.
//!
//! All module declarations live in lib.rs.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::info;

use fluxbase_queue::{api, config, db, dispatch, state, worker};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::config::init();

    let config = config::config::load();
    let pool = db::connection::init_pool(&config.database_url, config.worker_concurrency).await?;

    // Build an HTTP-backed ApiDispatch so the worker can ship spans to
    // flux.platform_logs via the control-plane API (multi-process mode).
    let api_dispatch: Arc<dyn job_contract::dispatch::ApiDispatch> =
        Arc::new(dispatch::HttpApiDispatch {
            client:  reqwest::Client::new(),
            api_url: config.api_url.clone(),
            token:   config.service_token.clone(),
        });

    // Shutdown channel: send () to signal all background tasks to stop.
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    tokio::spawn(worker::worker::start(
        pool.clone(),
        Arc::clone(&api_dispatch),
        config.runtime_url.clone(),
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
