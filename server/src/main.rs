//! Monolithic Flux server — single binary, single port.
//!
//! All five services (API, Runtime, Gateway, Data-Engine, Queue) run in one
//! OS process.  Service boundaries are enforced by the compile-time dispatch
//! traits (`RuntimeDispatch`, `ApiDispatch`) rather than by HTTP.
//!
//! Architecture:
//!   ```
//!   :4000
//!    ├─ /flux/api/*         → api::create_app     (management, secrets, logs, …)
//!    ├─ /flux/data-engine/* → data_engine routes   (query, mutations, history, …)
//!    ├─ /flux/queue/*       → queue routes         (jobs, stats, retry, …)
//!    ├─ /execute            → runtime handler      (queue worker → runtime, loopback)
//!    ├─ /flux/dev/invoke/*  → dev invoke shortcut
//!    ├─ /flux/              → dashboard SPA        (static assets + SPA fallback)
//!    └─ /{*path}            → gateway router       (function invocation, auth, rate-limit)
//!                                │
//!                                └─ InProcessRuntimeDispatch  (no HTTP)
//!                                        │
//!                                        └─ runtime::execute::service::invoke
//!   ```

mod dispatch;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use sqlx::postgres::PgPoolOptions;
use tokio::net::TcpListener;
use tokio::sync::watch;
use tracing::info;

use dispatch::{InProcessApiDispatch, InProcessDataEngineDispatch, InProcessQueueDispatch, InProcessRuntimeDispatch};
use gateway::state::GatewayState;
use job_contract::dispatch::{ApiDispatch, DataEngineDispatch, QueueDispatch};
use runtime::engine::executor::PoolDispatchers;
use runtime::secrets::client::SecretsClient;
use runtime::engine::pool::IsolatePool;
use runtime::engine::wasm_pool::WasmPool;
use runtime::bundle::cache::BundleCache;
use runtime::schema::cache::SchemaCache;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ── Observability ─────────────────────────────────────────────────────
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load .env if present (silently ignored in production).
    dotenvy::dotenv().ok();

    // Install the Prometheus metrics recorder once at startup.
    // The scrape endpoint /internal/metrics is registered in gateway::create_router.
    gateway::metrics::init_prometheus();

    // ── Config ────────────────────────────────────────────────────────────
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "4000".to_string())
        .parse::<u16>()
        .expect("PORT must be a valid u16");

    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL is required");

    let service_token = std::env::var("INTERNAL_SERVICE_TOKEN")
        .unwrap_or_else(|_| {
            if std::env::var("FLUX_ENV").as_deref() == Ok("production") {
                panic!(
                    "[Flux] INTERNAL_SERVICE_TOKEN must be set in production. \
                     The server cannot start without it."
                );
            }
            tracing::warn!(
                "[Flux] INTERNAL_SERVICE_TOKEN not set — using insecure default 'dev-service-token'. \
                 Set INTERNAL_SERVICE_TOKEN in production."
            );
            "dev-service-token".to_string()
        });

    let isolate_workers = std::env::var("ISOLATE_WORKERS")
        .unwrap_or_else(|_| "4".to_string())
        .parse::<usize>()
        .unwrap_or(4);

    let max_request_size_bytes = std::env::var("MAX_REQUEST_SIZE_BYTES")
        .unwrap_or_else(|_| "10485760".to_string())
        .parse::<usize>()
        .unwrap_or(10 * 1024 * 1024);

    let rate_limit_per_sec = std::env::var("RATE_LIMIT_PER_SEC")
        .unwrap_or_else(|_| "50".to_string())
        .parse::<u32>()
        .unwrap_or(50);

    let local_mode = std::env::var("LOCAL_MODE")
        .or_else(|_| std::env::var("FLUX_LOCAL"))
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    let queue_worker_concurrency: usize = std::env::var("QUEUE_WORKER_CONCURRENCY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);

    let queue_poll_interval_ms: u64 = std::env::var("WORKER_POLL_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);

    let queue_timeout_check_ms: u64 = std::env::var("JOB_TIMEOUT_CHECK_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30_000);

    // ── Shared HTTP client ────────────────────────────────────────────────
    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(30))
        .build()?;

    // ── Database pools ────────────────────────────────────────────────────
    //
    // Two pools with different search_path settings:
    //   pool    → flux, public          (API, Gateway, Queue, Runtime)
    //   de_pool → fluxbase_internal, flux, public  (Data-Engine)
    api::config::init();
    let pool = api::db::connection::init_pool().await?;
    info!("Server connected to database (flux pool)");

    let de_pool = PgPoolOptions::new()
        .max_connections(
            std::env::var("DE_DB_POOL_SIZE")
                .ok()
                .and_then(|v| v.parse::<u32>().ok())
                .unwrap_or(20),
        )
        .after_connect(|conn, _meta| Box::pin(async move {
            sqlx::query("SET search_path = fluxbase_internal, flux, public")
                .execute(conn)
                .await?;
            Ok(())
        }))
        .connect(&database_url)
        .await?;
    info!("Server connected to database (data-engine pool)");

    // ── Shutdown channel ──────────────────────────────────────────────────
    let (shutdown_tx, shutdown_rx) = watch::channel(());

    // ── API AppState ──────────────────────────────────────────────────────
    let base_url = format!("http://localhost:{}", port);
    let api_state = Arc::new(api::AppState {
        pool:            pool.clone(),
        http_client:     http_client.clone(),
        data_engine_url: format!("{}/flux/data-engine", base_url),
        gateway_url:     base_url.clone(),
        runtime_url:     base_url.clone(),
        functions_dir: std::env::var("FLUX_FUNCTIONS_DIR")
            .unwrap_or_else(|_| "./flux-functions".to_string()),
    });

    // ── In-process API dispatch (no HTTP) ─────────────────────────────────
    let api_dispatch: Arc<dyn ApiDispatch> = Arc::new(InProcessApiDispatch {
        state: Arc::clone(&api_state),
    });
    // Extra refs needed by Queue and its worker (before runtime_state takes ownership).
    let api_dispatch_for_queue = Arc::clone(&api_dispatch);
    let api_dispatch_for_worker = Arc::clone(&api_dispatch);

    // ── In-process Queue + Data-Engine dispatch (no HTTP) ─────────────────
    let queue_dispatch: Arc<dyn QueueDispatch> = Arc::new(InProcessQueueDispatch {
        pool: pool.clone(),
    });
    let de_dispatch: Arc<dyn DataEngineDispatch> = Arc::new(InProcessDataEngineDispatch::new(
        de_pool.clone(),
        env_parse("STATEMENT_TIMEOUT_MS", 5000),
    ));

    // ── PoolDispatchers — shared by V8 ops and WASM host functions ────────
    let runtime_lock = Arc::new(std::sync::OnceLock::new());
    let dispatchers = PoolDispatchers {
        api:         Arc::clone(&api_dispatch),
        queue:       Arc::clone(&queue_dispatch),
        data_engine: Arc::clone(&de_dispatch),
        runtime:     Arc::clone(&runtime_lock),
    };

    // ── Runtime AppState ──────────────────────────────────────────────────
    let request_timeout_secs: u64 = std::env::var("REQUEST_TIMEOUT_SECONDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(30);
    let runtime_state = Arc::new(runtime::AppState {
        secrets_client: SecretsClient::new(Arc::clone(&api_dispatch)),
        http_client:    http_client.clone(),
        api:            api_dispatch,
        queue:          queue_dispatch,
        data_engine:    de_dispatch,
        service_token:  service_token.clone(),
        bundle_cache:   BundleCache::new(100),
        schema_cache:   SchemaCache::new(200),
        isolate_pool:   IsolatePool::new(isolate_workers, request_timeout_secs, dispatchers.clone()),
        wasm_pool:      WasmPool::default_sized(),
        dispatchers:    dispatchers.clone(),
    });

    // ── In-process runtime dispatch ───────────────────────────────────────
    let runtime_dispatch = Arc::new(InProcessRuntimeDispatch {
        state: Arc::clone(&runtime_state),
    });
    // Fill the OnceLock so V8 ctx.function.invoke() can reach back into the runtime.
    let _ = runtime_lock.set(
        Arc::clone(&runtime_dispatch) as Arc<dyn job_contract::dispatch::RuntimeDispatch>,
    );
    // Keep a ref for the dev invoke endpoint before moving into gateway_state.
    let runtime_dispatch_ref: Arc<dyn job_contract::dispatch::RuntimeDispatch> =
        Arc::clone(&runtime_dispatch) as Arc<dyn job_contract::dispatch::RuntimeDispatch>;

    // ── Data-Engine AppState ──────────────────────────────────────────────
    let de_cfg = data_engine::config::Config {
        database_url:           database_url.clone(),
        port:                   port,
        default_query_limit:    env_parse("DEFAULT_QUERY_LIMIT", 100),
        max_query_limit:        env_parse("MAX_QUERY_LIMIT", 5000),
        runtime_url:            base_url.clone(),
        max_query_complexity:   env_parse("MAX_QUERY_COMPLEXITY", 1000),
        query_timeout_ms:       env_parse("QUERY_TIMEOUT_MS", 30_000),
        statement_timeout_ms:   env_parse("STATEMENT_TIMEOUT_MS", 5000),
        max_nest_depth:         env_parse("MAX_NEST_DEPTH", 6),
        record_retention_days:  env_parse("RECORD_RETENTION_DAYS", 30),
        error_retention_days:   std::env::var("ERROR_RETENTION_DAYS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        retention_job_hour:     env_parse("RETENTION_JOB_HOUR", 3),
    };
    let de_state = Arc::new(
        data_engine::state::AppState::new(de_pool.clone(), &de_cfg).await,
    );

    // ── Queue AppState ────────────────────────────────────────────────────
    let queue_state = Arc::new(
        fluxbase_queue::state::AppState::new(
            pool.clone(),
            api_dispatch_for_queue,
        ),
    );

    // ── Gateway state ─────────────────────────────────────────────────────
    let snapshot = gateway::snapshot::GatewaySnapshot::new(
        pool.clone(),
        database_url.clone(),
    );
    if let Err(e) = snapshot.refresh().await {
        tracing::warn!("Initial snapshot fetch failed (will retry): {:?}", e);
    }
    gateway::snapshot::GatewaySnapshot::start_notify_listener(snapshot.clone());

    let jwks_cache = gateway::auth::JwksCache::new(http_client.clone());

    let gateway_state = Arc::new(GatewayState {
        db_pool:                pool.clone(),
        runtime:                runtime_dispatch,
        snapshot,
        jwks_cache,
        max_request_size_bytes,
        rate_limit_per_sec,
        local_mode,
    });

    // ── Background workers ────────────────────────────────────────────────

    // Queue worker — polls for pending jobs and dispatches them to the runtime.
    let runtime_dispatch_for_worker: Arc<dyn job_contract::dispatch::RuntimeDispatch> =
        Arc::clone(&runtime_dispatch_ref);
    tokio::spawn(fluxbase_queue::worker::worker::start(
        pool.clone(),
        api_dispatch_for_worker,
        runtime_dispatch_for_worker,
        service_token.clone(),
        queue_worker_concurrency,
        queue_poll_interval_ms,
        shutdown_rx.clone(),
    ));
    info!("Queue worker started (concurrency={})", queue_worker_concurrency);

    // Queue timeout recovery — rescues stuck jobs.
    tokio::spawn(fluxbase_queue::worker::timeout_recovery::run(
        pool.clone(),
        queue_timeout_check_ms,
        shutdown_rx.clone(),
    ));

    // Data-engine cache invalidation — keeps schema/plan caches in sync via LISTEN/NOTIFY.
    data_engine::cache::invalidation::start_listener(
        Arc::clone(&de_state),
        database_url.clone(),
    );

    // Data-engine cron scheduler — fires overdue cron jobs.
    let cron_pool = Arc::new(de_pool.clone());
    let cron_http = Arc::new(http_client.clone());
    let cron_url = base_url.clone();
    tokio::spawn(async move {
        data_engine::cron::worker::run(cron_pool, cron_http, cron_url).await;
    });

    // Data-engine retention — daily hard-delete of old execution records.
    let ret_pool = Arc::new(de_pool.clone());
    let ret_cfg = data_engine::retention::RetentionConfig {
        record_retention_days: de_cfg.record_retention_days,
        error_retention_days:  de_cfg.error_retention_days,
        job_hour_utc:          de_cfg.retention_job_hour,
    };
    tokio::spawn(async move {
        data_engine::retention::worker::run(ret_pool, ret_cfg).await;
    });

    // ── Router ────────────────────────────────────────────────────────────
    let dashboard_dir = std::env::var("FLUX_DASHBOARD_DIR")
        .unwrap_or_else(|_| "dashboard/out".to_string());
    let dashboard_index = format!("{}/index.html", dashboard_dir);

    // Dev invoke state — runtime dispatch for unregistered calls.
    let dev_invoke_state = Arc::new(DevInvokeState {
        runtime:    Arc::clone(&runtime_dispatch_ref),
    });

    let dev_invoke_router = axum::Router::new()
        .route("/flux/dev/invoke/{name}", axum::routing::post(dev_invoke_handler))
        .with_state(dev_invoke_state);

    // Runtime execute endpoint — queue worker POSTs here via loopback.
    // Also exposes cache invalidation so the API can flush bundles after deploy.
    let runtime_execute_router = axum::Router::new()
        .route("/execute", axum::routing::post(runtime::execute::handler::execute_handler))
        .route("/internal/cache/invalidate", axum::routing::post(runtime::execute::invalidate::invalidate_cache_handler))
        .with_state(runtime_state);

    let app = axum::Router::new()
        .nest("/flux/api", api::create_app((*api_state).clone()))
        .nest("/flux/data-engine", data_engine::api::routes::build(de_state))
        .nest("/flux/queue", fluxbase_queue::api::routes::routes(queue_state))
        .merge(runtime_execute_router)
        .merge(dev_invoke_router)
        .nest_service(
            "/flux",
            tower_http::services::ServeDir::new(&dashboard_dir)
                .not_found_service(tower_http::services::ServeFile::new(&dashboard_index)),
        )
        .merge(gateway::create_router(gateway_state));

    // ── Listen ────────────────────────────────────────────────────────────
    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    // Optional TLS — set FLUX_TLS_CERT and FLUX_TLS_KEY to paths of PEM files.
    // When the env vars are absent the server falls back to plain HTTP.
    let tls_cert = std::env::var("FLUX_TLS_CERT").ok();
    let tls_key  = std::env::var("FLUX_TLS_KEY").ok();

    match (tls_cert, tls_key) {
        (Some(cert_path), Some(key_path)) => {
            info!(port, cert = %cert_path, "Flux server listening (TLS)");
            let tls_config = axum_server::tls_rustls::RustlsConfig::from_pem_file(
                &cert_path,
                &key_path,
            )
            .await
            .expect("Failed to load TLS certificate/key");
            axum_server::bind_rustls(addr, tls_config)
                .serve(app.into_make_service())
                .await?;
        }
        _ => {
            info!(port, "Flux monolithic server listening (all 5 services)");
            let listener = TcpListener::bind(addr).await?;
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_signal().await;
                    info!("Shutdown signal received — stopping background workers");
                    let _ = shutdown_tx.send(());
                })
                .await?;
        }
    }

    Ok(())
}

/// Parse an env var with a default value.
fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Resolves on SIGTERM (Unix) or Ctrl-C.
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
}

// ── Dev invoke endpoint ───────────────────────────────────────────────────────
//
// POST /flux/dev/invoke/:name
//
// Calls a function by name directly via the in-process runtime — no route
// registration required.  Intended only for `flux invoke` in local dev.

struct DevInvokeState {
    runtime:    Arc<dyn job_contract::dispatch::RuntimeDispatch>,
}

async fn dev_invoke_handler(
    axum::extract::State(state): axum::extract::State<Arc<DevInvokeState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
    axum::Json(payload): axum::Json<serde_json::Value>,
) -> axum::response::Response {
    use axum::{http::StatusCode, Json, response::IntoResponse};
    use job_contract::dispatch::ExecuteRequest;

    let req = ExecuteRequest {
        function_id:    name.clone(),
        payload:        payload.clone(),
        execution_seed: None,
        request_id:     Some(uuid::Uuid::new_v4().to_string()),
        parent_span_id: None,
        runtime_hint:   None,
        user_id:        None,
        jwt_claims:     None,
    };

    match state.runtime.execute(req).await {
        Ok(resp) => {
            let status = StatusCode::from_u16(resp.status)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, Json(resp.body)).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": "invoke_failed", "message": e })),
        ).into_response(),
    }
}
