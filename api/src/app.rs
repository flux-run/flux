//! Core application state and router construction.
//!
//! Extracted from `main.rs` so both the standalone binary *and* the monolithic
//! `server` crate can import these items via `api::AppState` / `api::create_app`.

use axum::{
    middleware as axum_middleware,
    routing::{any, delete, get, post, put},
    Json,
    Router,
};
use tower_http::cors::{CorsLayer, AllowOrigin};
use axum::http::{HeaderValue, Method, header};
use tracing::info;
use uuid::Uuid;

use crate::auth;
use crate::services;
use crate::middleware;
use crate::secrets;
use crate::logs;
use crate::routes;

// ── AppState ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub pool:             sqlx::PgPool,
    pub http_client:      reqwest::Client,
    pub data_engine_url:  String,
    pub gateway_url:      String,
    pub storage:          services::storage::StorageService,
    /// Fixed tenant UUID used in local / single-tenant mode.
    pub local_tenant_id:  Uuid,
    /// Default project UUID; can be overridden by FLUX_PROJECT_ID env var.
    pub local_project_id: Uuid,
}

impl axum::extract::FromRef<AppState> for sqlx::PgPool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

// ── CORS ──────────────────────────────────────────────────────────────────────

/// Build the CORS layer from the `ALLOWED_ORIGINS` environment variable.
///
/// `ALLOWED_ORIGINS` is a comma-separated list, e.g.:
///   `http://localhost:5173,https://app.example.com`
///
/// Defaults to `http://localhost:5173` when unset.
pub fn build_cors() -> CorsLayer {
    let raw = std::env::var("ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:5173".to_string());

    let origins: Vec<HeaderValue> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<HeaderValue>().expect("invalid ALLOWED_ORIGINS entry"))
        .collect();

    info!("CORS allowed origins: {:?}", origins);

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            header::ACCEPT,
            "x-flux-project".parse().unwrap(),
            "x-request-id".parse().unwrap(),
        ])
        .allow_credentials(true)
        .max_age(std::time::Duration::from_secs(3600))
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn create_app(state: AppState) -> Router {
    // ── Internal: service-token protected ─────────────────────────────────
    let internal = Router::new()
        .route("/secrets",           get(secrets::routes::get_internal_runtime_secrets))
        .route("/bundle",            get(routes::deployments::get_internal_bundle))
        .route("/introspect",        get(routes::introspect::get_project_introspect))
        .route("/introspect/manifest", get(routes::manifest::get_manifest))
        .route("/logs",              post(logs::routes::create_log).get(logs::routes::list_logs))
        .route("/functions/resolve", get(routes::functions::resolve_function))
        .route("/cache/invalidate",  post(routes::system::cache_invalidate))
        .layer(axum_middleware::from_fn(middleware::internal_auth::require_service_token));

    // ── API: optional FLUX_API_KEY guard ──────────────────────────────────
    let api = Router::new()
        // Functions
        .route("/functions",         get(routes::functions::list_functions).post(routes::functions::create_function))
        .route("/functions/{id}",    get(routes::functions::get_function).delete(routes::functions::delete_function))
        // Deploy
        .route("/functions/deploy",  post(routes::deployments::deploy_function_cli)
            .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)))
        .route("/deployments",               post(routes::deployments::create_deployment))
        .route("/deployments/list/{id}",     get(routes::deployments::list_deployments))
        .route("/deployments/{id}/activate/{version}", post(routes::deployments::activate_deployment))
        // Secrets
        .route("/secrets",           get(secrets::routes::list_secrets).post(secrets::routes::create_secret))
        .route("/secrets/{key}",     put(secrets::routes::update_secret).delete(secrets::routes::delete_secret))
        // Logs + traces
        .route("/logs",              get(logs::routes::list_project_logs))
        .route("/traces/{request_id}", get(logs::routes::get_trace))
        .route("/traces",            get(logs::routes::list_traces))
        // Gateway routes
        .route("/gateway/routes",               get(routes::gateway_routes::list_gateway_routes).post(routes::gateway_routes::create_gateway_route))
        .route("/gateway/routes/{id}",          get(routes::stubs::get_gateway_route_by_id)
                                                .patch(routes::gateway_routes::update_gateway_route)
                                                .delete(routes::gateway_routes::delete_gateway_route))
        .route("/gateway/middleware",            post(routes::stubs::gateway_middleware_create))
        .route("/gateway/middleware/{route}/{type}", delete(routes::stubs::gateway_middleware_delete))
        .route("/gateway/routes/{id}/rate-limit", put(routes::stubs::gateway_route_rate_limit_set)
                                                 .delete(routes::stubs::gateway_route_rate_limit_delete))
        .route("/gateway/routes/{id}/cors",      get(routes::stubs::gateway_route_cors_get)
                                                 .put(routes::stubs::gateway_route_cors_set))
        // Schema / SDK / spec
        .route("/schema/graph",      get(routes::schema::graph))
        .route("/sdk/schema",        get(routes::sdk::schema))
        .route("/sdk/typescript",    get(routes::sdk::typescript))
        .route("/sdk/manifest",      get(routes::manifest::get_manifest))
        .route("/openapi.json",      get(routes::openapi::spec))
        .route("/spec",              get(routes::spec::project_spec))
        // Data Engine + Files proxy
        .route("/db/{*path}",        any(routes::data_engine::proxy_handler))
        .route("/files/{*path}",     any(routes::data_engine::proxy_handler))
        // ── API Keys ──────────────────────────────────────────────────────────
        .route("/api-keys",              get(routes::stubs::api_keys_list).post(routes::stubs::api_key_create))
        .route("/api-keys/{id}",         delete(routes::stubs::api_key_delete))
        .route("/api-keys/{id}/rotate",  post(routes::stubs::api_key_rotate))
        // ── Records ───────────────────────────────────────────────────────────
        .route("/records/export",        get(routes::records::records_export))
        .route("/records/count",         get(routes::records::records_count))
        .route("/records/prune",         delete(routes::records::records_prune))
        // ── Monitor ───────────────────────────────────────────────────────────
        .route("/monitor/status",        get(routes::stubs::monitor_status))
        .route("/monitor/metrics",       get(routes::stubs::monitor_metrics))
        .route("/monitor/alerts",        get(routes::stubs::monitor_alerts_list).post(routes::stubs::monitor_alert_create))
        .route("/monitor/alerts/{id}",   delete(routes::stubs::monitor_alert_delete))
        // ── Events ────────────────────────────────────────────────────────────
        .route("/events",                post(routes::stubs::events_publish))
        .route("/events/subscriptions",  get(routes::stubs::events_subscriptions_list).post(routes::stubs::events_subscribe))
        .route("/events/subscriptions/{id}", delete(routes::stubs::events_unsubscribe))
        // ── Queue management ──────────────────────────────────────────────────
        .route("/queues",                get(routes::stubs::queues_list).post(routes::stubs::queue_create))
        .route("/queues/{name}",         get(routes::stubs::queue_get).delete(routes::stubs::queue_delete))
        .route("/queues/{name}/messages",post(routes::stubs::queue_publish_message))
        .route("/queues/{name}/bindings",get(routes::stubs::queue_bindings_list).post(routes::stubs::queue_binding_create))
        .route("/queues/{name}/purge",   post(routes::stubs::queue_purge))
        .route("/queues/{name}/dlq",     get(routes::stubs::queue_dlq_list))
        .route("/queues/{name}/dlq/replay", post(routes::stubs::queue_dlq_replay))
        // ── Schedules ─────────────────────────────────────────────────────────
        .route("/schedules",             get(routes::stubs::schedules_list).post(routes::stubs::schedule_create))
        .route("/schedules/{name}",      delete(routes::stubs::schedule_delete))
        .route("/schedules/{name}/pause",   post(routes::stubs::schedule_pause))
        .route("/schedules/{name}/resume",  post(routes::stubs::schedule_resume))
        .route("/schedules/{name}/run",     post(routes::stubs::schedule_run_now))
        .route("/schedules/{name}/history", get(routes::stubs::schedule_history))
        // ── Agents ────────────────────────────────────────────────────────────
        .route("/agents",                get(routes::agents::agents_list).post(routes::agents::agent_deploy))
        .route("/agents/{name}",         get(routes::agents::agent_get).delete(routes::agents::agent_delete))
        .route("/agents/{name}/run",     post(routes::agents::agent_run))
        .route("/agents/{name}/simulate",post(routes::agents::agent_simulate))
        // ── Environments ──────────────────────────────────────────────────────
        .route("/environments",          get(routes::stubs::environments_list).post(routes::stubs::environment_create))
        .route("/environments/clone",    post(routes::stubs::environments_clone))
        .route("/environments/{name}",   delete(routes::stubs::environment_delete))
        // Auth middleware injects RequestContext
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::auth::require_auth,
        ));

    // ── Auth: public routes (no require_auth guard) ───────────────────────
    let auth = Router::new()
        .route("/auth/setup",          post(auth::routes::setup))
        .route("/auth/login",          post(auth::routes::login))
        .route("/auth/logout",         post(auth::routes::logout))
        .route("/auth/me",             get(auth::routes::me))
        .route("/auth/users",          get(auth::routes::list_users).post(auth::routes::create_user))
        .route("/auth/users/{id}",     delete(auth::routes::delete_user));

    Router::new()
        .merge(api)
        .merge(auth)
        .nest("/internal", internal)
        // ── Execution-plane guard ──────────────────────────────────────────
        .route("/run",                         any(routes::system::execution_not_allowed))
        .route("/run/{*path}",                 any(routes::system::execution_not_allowed))
        .route("/invoke",                      any(routes::system::execution_not_allowed))
        .route("/invoke/{*path}",              any(routes::system::execution_not_allowed))
        .route("/execute",                     any(routes::system::execution_not_allowed))
        .route("/execute/{*path}",             any(routes::system::execution_not_allowed))
        .route("/functions/{name}/run",        any(routes::system::execution_not_allowed))
        .route("/functions/{name}/invoke",     any(routes::system::execution_not_allowed))
        // ── Swagger UI ────────────────────────────────────────────────────
        .route("/openapi/ui",                  get(routes::openapi::ui))
        // ── Utility ───────────────────────────────────────────────────────
        .route("/health", get(|| async { Json(serde_json::json!({ "status": "ok" })) }))
        .route("/version", get(|| async {
            Json(serde_json::json!({
                "service":     "api",
                "commit":      std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
                "build_time":  std::env::var("BUILD_TIME").unwrap_or_else(|_| "unknown".to_string()),
            }))
        }))
        .fallback(|req: axum::extract::Request| async move {
            tracing::warn!("404: {} {}", req.method(), req.uri().path());
            (
                axum::http::StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error":   "not_found",
                    "message": "The requested path does not exist",
                    "code":    404,
                    "path":    req.uri().path().to_string(),
                })),
            )
        })
        .layer(build_cors())
        .layer(axum_middleware::from_fn(middleware::request_id::request_id_middleware))
        .layer(axum::extract::DefaultBodyLimit::max(1 * 1024 * 1024))
        .with_state(state)
}

// ── Local mode seed ───────────────────────────────────────────────────────────

/// Idempotently seeds the local tenant and project rows so FK constraints are
/// satisfied even on a fresh database.  Called once at startup before the server
/// starts accepting requests.
pub async fn init_local_mode(pool: &sqlx::PgPool) -> Result<(Uuid, Uuid), sqlx::Error> {
    const LOCAL_TENANT_ID: &str = "00000000-0000-0000-0000-000000000001";
    const LOCAL_PROJECT_ID: &str = "00000000-0000-0000-0000-000000000002";

    let tenant_id = Uuid::parse_str(LOCAL_TENANT_ID).unwrap();

    sqlx::query(
        "INSERT INTO tenants (id, name) VALUES ($1, 'local') ON CONFLICT (id) DO NOTHING"
    )
    .bind(tenant_id)
    .execute(pool)
    .await?;

    let project_id = std::env::var("FLUX_PROJECT_ID")
        .ok()
        .and_then(|s| Uuid::parse_str(&s).ok())
        .unwrap_or_else(|| Uuid::parse_str(LOCAL_PROJECT_ID).unwrap());

    sqlx::query(
        "INSERT INTO projects (id, tenant_id, name) VALUES ($1, $2, $3) ON CONFLICT (id) DO NOTHING"
    )
    .bind(project_id)
    .bind(tenant_id)
    .bind("default")
    .execute(pool)
    .await?;

    info!(
        "Local mode: tenant_id={} project_id={}",
        tenant_id, project_id
    );

    Ok((tenant_id, project_id))
}
