//! Queue entry point — thin startup wrapper.
//!
//! All module declarations live in lib.rs.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::info;

use fluxbase_queue::{api, config, db, dispatch, state, worker};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::config::init();

    let config = config::config::load();
    let pool = db::connection::init_pool(&config.database_url).await?;

    // Build an HTTP-backed ApiDispatch so the worker can ship spans to
    // flux.platform_logs via the control-plane API (multi-process mode).
    let api_dispatch: Arc<dyn job_contract::dispatch::ApiDispatch> =
        Arc::new(dispatch::HttpApiDispatch {
            client:  reqwest::Client::new(),
            api_url: config.api_url.clone(),
            token:   config.service_token.clone(),
        });

    tokio::spawn(worker::worker::start(
        pool.clone(),
        Arc::clone(&api_dispatch),
        config.runtime_url.clone(),
        config.service_token.clone(),
        config.worker_concurrency,
        config.poll_interval_ms,
    ));

    tokio::spawn(worker::timeout_recovery::run(
        pool.clone(),
        config.job_timeout_check_interval_ms,
    ));

    let app_state = Arc::new(state::AppState::new(pool, api_dispatch));
    let app = api::routes::routes(app_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    info!("Starting Fluxbase Queue on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
