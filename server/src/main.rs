//! Monolithic Flux server — single binary, single port.
//!
//! All five services (API, Runtime, Gateway, Data-Engine, Queue) run in one
//! OS process.  Service boundaries are enforced by the compile-time dispatch
//! traits (`RuntimeDispatch`, `ApiDispatch`) rather than by HTTP.
//!
//! Architecture:
//!   ```
//!   :4000
//!    ├─ /flux/api/* → api::create_app  (management, secrets, logs, …)
//!    ├─ /flux/      → dashboard SPA    (static assets + SPA fallback)
//!    └─ /{*path}    → gateway router   (function invocation, auth, rate-limit)
//!                          │
//!                          └─ InProcessRuntimeDispatch  (no HTTP)
//!                                  │
//!                                  └─ runtime::execute::service::invoke
//!   ```

mod dispatch;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpListener;
use tracing::info;

use dispatch::{InProcessApiDispatch, InProcessRuntimeDispatch};
use gateway::state::GatewayState;
use job_contract::dispatch::ApiDispatch;
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

    let queue_url = std::env::var("QUEUE_URL")
        .unwrap_or_else(|_| "http://localhost:8084".to_string());

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

    // ── Shared HTTP client ────────────────────────────────────────────────
    let http_client = reqwest::Client::builder()
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(Duration::from_secs(90))
        .tcp_keepalive(Duration::from_secs(30))
        .build()?;

    // ── Database pool (shared by all services) ────────────────────────────
    api::config::init();
    let pool = api::db::connection::init_pool().await?;
    info!("Server connected to database");

    // ── API AppState ──────────────────────────────────────────────────────
    let (local_tenant_id, local_project_id) =
        api::app::init_local_mode(&pool).await?;

    let api_state = Arc::new(api::AppState {
        pool:            pool.clone(),
        http_client:     http_client.clone(),
        data_engine_url: std::env::var("DATA_ENGINE_URL")
            .unwrap_or_else(|_| "http://localhost:8082".to_string()),
        gateway_url: format!("http://localhost:{}", port),
        local_tenant_id,
        local_project_id,
    });

    // ── In-process API dispatch (no HTTP) ─────────────────────────────────
    let api_dispatch: Arc<dyn ApiDispatch> = Arc::new(InProcessApiDispatch {
        state: Arc::clone(&api_state),
    });

    // ── Runtime AppState ──────────────────────────────────────────────────
    let runtime_state = Arc::new(runtime::AppState {
        secrets_client: SecretsClient::new(Arc::clone(&api_dispatch)),
        http_client:    http_client.clone(),
        api:            api_dispatch,
        api_url:        format!("http://localhost:{}/flux/api", port),
        queue_url:      queue_url.clone(),
        service_token:  service_token.clone(),
        bundle_cache:   BundleCache::new(100),
        schema_cache:   SchemaCache::new(200),
        isolate_pool:   IsolatePool::new(isolate_workers),
        wasm_pool:      WasmPool::default_sized(),
    });

    // ── In-process runtime dispatch ───────────────────────────────────────
    let runtime_dispatch = Arc::new(InProcessRuntimeDispatch {
        state: Arc::clone(&runtime_state),
    });
    // Keep a ref for the dev invoke endpoint before moving into gateway_state.
    let runtime_dispatch_ref: Arc<dyn job_contract::dispatch::RuntimeDispatch> =
        Arc::clone(&runtime_dispatch) as Arc<dyn job_contract::dispatch::RuntimeDispatch>;

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

    // ── Router ────────────────────────────────────────────────────────────
    // /flux/api/* → API management (secrets, logs, deployments, …)
    // /flux/dev/  → Dev-only shortcuts (invoke without route registration)
    // /flux/      → Dashboard SPA  (static assets + SPA index.html fallback)
    // /{*path}    → Gateway function invocation (wildcard, lowest priority)
    let dashboard_dir = std::env::var("FLUX_DASHBOARD_DIR")
        .unwrap_or_else(|_| "dashboard/out".to_string());
    let dashboard_index = format!("{}/index.html", dashboard_dir);

    // Dev invoke state — runtime dispatch + project_id for unregistered calls.
    let dev_invoke_state = Arc::new(DevInvokeState {
        runtime:    Arc::clone(&runtime_dispatch_ref),
        project_id: api_state.local_project_id,
    });

    let dev_invoke_router = axum::Router::new()
        .route("/flux/dev/invoke/{name}", axum::routing::post(dev_invoke_handler))
        .with_state(dev_invoke_state);

    let app = axum::Router::new()
        .nest("/flux/api", api::create_app((*api_state).clone()))
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
            axum_server::bind_rustls(addr, tls_config).serve(app.into_make_service()).await?;
        }
        _ => {
            info!(port, "Flux monolithic server listening");
            let listener = TcpListener::bind(addr).await?;
            axum::serve(listener, app).await?;
        }
    }

    Ok(())
}

// ── Dev invoke endpoint ───────────────────────────────────────────────────────
//
// POST /flux/dev/invoke/:name
//
// Calls a function by name directly via the in-process runtime — no route
// registration required.  Intended only for `flux invoke` in local dev.

struct DevInvokeState {
    runtime:    Arc<dyn job_contract::dispatch::RuntimeDispatch>,
    project_id: uuid::Uuid,
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
        project_id:     Some(state.project_id),
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
