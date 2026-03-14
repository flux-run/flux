//! Data-engine entry point — thin startup wrapper.
//!
//! All module declarations live in lib.rs.

use std::sync::Arc;

use data_engine::{api, cache, config, cron, db, retention, state, telemetry};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    config::init();
    telemetry::init();
    let cfg = config::load();

    tracing::info!("connecting to database...");
    let pool = db::connection::init_pool_with_identity_log(&cfg.database_url, "platform").await;

    let app_state = Arc::new(state::AppState::new(pool.clone(), &cfg).await);

    let worker_pool = Arc::new(pool);
    let worker_http = Arc::new(reqwest::Client::new());
    let worker_runtime_url = cfg.runtime_url.clone();

    // Cache invalidation listener — keeps all instances in sync via Postgres LISTEN/NOTIFY.
    cache::invalidation::start_listener(Arc::clone(&app_state), cfg.database_url.clone());

    // Cron scheduler worker
    let cron_pool = Arc::clone(&worker_pool);
    let cron_http = Arc::clone(&worker_http);
    let cron_url = worker_runtime_url.clone();
    tokio::spawn(async move {
        cron::worker::run(cron_pool, cron_http, cron_url).await;
    });

    // Retention job — daily hard-delete of old execution records
    let ret_pool = Arc::clone(&worker_pool);
    let ret_cfg = retention::RetentionConfig {
        record_retention_days: cfg.record_retention_days,
        error_retention_days:  cfg.error_retention_days,
        job_hour_utc:          cfg.retention_job_hour,
    };
    tokio::spawn(async move {
        retention::worker::run(ret_pool, ret_cfg).await;
    });

    let app = api::routes::build(app_state);

    let addr = format!("0.0.0.0:{}", cfg.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("data-engine listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}
