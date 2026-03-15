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
use crate::auth;
use crate::middleware;
use crate::secrets;
use crate::logs;
use crate::routes;
use api_contract::routes as R;

// ── AppState ──────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub pool:             sqlx::PgPool,
    pub http_client:      reqwest::Client,
    pub data_engine_url:  String,
    pub gateway_url:      String,
    /// URL of the Runtime service — used by cache invalidation after a deployment
    /// (`POST /internal/cache/invalidate`).  In the monolith this is the same
    /// base URL as everything else (port 4000).
    pub runtime_url:      String,
    /// Directory where function bundles live on the filesystem.
    ///
    /// - Dev:        `{project_root}/.flux/build`  (set by `flux dev`)
    /// - Production: `/app/functions`              (baked into Docker image)
    /// - Override:   `FLUX_FUNCTIONS_DIR` env var
    pub functions_dir:    String,
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
            "x-request-id".parse().expect("x-request-id is a valid header name"),
        ])
        .allow_credentials(true)
        .max_age(std::time::Duration::from_secs(3600))
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn create_app(state: AppState) -> Router {
    // ── Internal: service-token protected ─────────────────────────────────
    let internal = Router::new()
        .route(R::internal::SECRETS_GET.under("/internal"),       get(secrets::routes::get_internal_runtime_secrets))
        .route(R::internal::BUNDLE_GET.under("/internal"),        get(routes::deployments::get_internal_bundle))
        .route(R::internal::INTROSPECT.under("/internal"),        get(routes::introspect::get_project_introspect))
        .route(R::internal::MANIFEST.under("/internal"),          get(routes::manifest::get_manifest))
        .route(R::internal::DB_MIGRATE.under("/internal"),        post(routes::db_migrate::apply_user_migration))
        .route(R::internal::DB_SCHEMA.under("/internal"),         post(routes::schema::push_schema))
        .route(R::internal::LOGS_CREATE.under("/internal"),       post(logs::routes::create_log).get(logs::routes::list_logs))
        .route(R::internal::FUNCTIONS_RESOLVE.under("/internal"), get(routes::functions::resolve_function))
        .route(R::internal::CACHE_INVALIDATE.under("/internal"),  post(routes::system::cache_invalidate))
        .route(R::internal::ROUTES_GET.under("/internal"),        get(routes::gateway_config::get_routes_for_gateway))
        .layer(axum_middleware::from_fn(middleware::internal_auth::require_service_token));

    // ── API: optional FLUX_API_KEY guard ──────────────────────────────────
    let api = Router::new()
        // Functions
        .route(R::functions::LIST.path,   get(routes::functions::list_functions).post(routes::functions::create_function))
        .route(R::functions::GET.path,    get(routes::functions::get_function).delete(routes::functions::delete_function))
        // Deploy
        .route(R::functions::DEPLOY.path, post(routes::deployments::deploy_function_cli)
            .layer(axum::extract::DefaultBodyLimit::max(60 * 1024 * 1024)))
        .route(R::deployments::CREATE.path,           post(routes::deployments::create_deployment))
        .route(R::deployments::LIST.path,             get(routes::deployments::list_deployments))
        .route(R::deployments::ACTIVATE.path,         post(routes::deployments::activate_deployment))
        .route(R::deployments::HASHES.path,           get(routes::deployments::get_deployment_hashes))
        .route(R::deployments::PROJECT_LIST.path,     get(routes::deployments::list_project_deployments)
                                                      .post(routes::deployments::create_project_deployment))
        .route(R::deployments::PROJECT_ROLLBACK.path, post(routes::deployments::rollback_project_deployment))
        // Secrets
        .route(R::secrets::LIST.path,   get(secrets::routes::list_secrets).post(secrets::routes::create_secret))
        .route(R::secrets::UPDATE.path, put(secrets::routes::update_secret).delete(secrets::routes::delete_secret))
        // Logs + traces
        .route(R::logs::LIST.path,        get(logs::routes::list_project_logs))
        .route(R::logs::TRACE_GET.path,   get(logs::routes::get_trace))
        .route(R::logs::TRACES_LIST.path, get(logs::routes::list_traces))
        // Gateway routes
        .route(R::gateway::ROUTES_LIST.path,       get(routes::gateway_routes::list_gateway_routes).post(routes::gateway_routes::create_gateway_route))
        .route(R::gateway::ROUTES_GET.path,        get(routes::gateway_routes::get_gateway_route_by_id)
                                                   .patch(routes::gateway_routes::update_gateway_route)
                                                   .delete(routes::gateway_routes::delete_gateway_route))
        .route(R::gateway::MIDDLEWARE_CREATE.path, post(routes::gateway_routes::create_middleware))
        .route(R::gateway::MIDDLEWARE_DELETE.path, delete(routes::gateway_routes::delete_middleware))
        .route(R::gateway::RATE_LIMIT_SET.path,    put(routes::gateway_routes::set_rate_limit)
                                                   .delete(routes::gateway_routes::delete_rate_limit))
        .route(R::gateway::CORS_GET.path,          get(routes::gateway_routes::get_cors)
                                                   .put(routes::gateway_routes::set_cors))
        // Schema / SDK / spec
        .route(R::sdk::SCHEMA_GRAPH.path, get(routes::schema::graph))
        .route(R::sdk::SDK_SCHEMA.path,   get(routes::sdk::schema))
        .route(R::sdk::SDK_TS.path,       get(routes::sdk::typescript))
        .route(R::sdk::MANIFEST.path,     get(routes::manifest::get_manifest))
        .route(R::sdk::OPENAPI.path,      get(routes::openapi::spec))
        .route(R::sdk::SPEC.path,         get(routes::spec::project_spec))
        // Migrations (must be before the /db/{*path} wildcard)
        .route(R::db::MIGRATE_APPLY.path,    post(routes::db_migrate::apply_migrations))
        .route(R::db::MIGRATE_ROLLBACK.path, post(routes::db_migrate::rollback_migration))
        .route(R::db::MIGRATE_STATUS.path,   get(routes::db_migrate::list_migrations))
        // Data Engine + Files proxy
        .route(R::proxy::DB.path,    any(routes::data_engine::proxy_handler))
        .route(R::proxy::FILES.path, any(routes::data_engine::proxy_handler))
        // API Keys
        .route(R::api_keys::LIST.path,   get(routes::api_keys::list_api_keys).post(routes::api_keys::create_api_key))
        .route(R::api_keys::DELETE.path, delete(routes::api_keys::delete_api_key))
        .route(R::api_keys::ROTATE.path, post(routes::api_keys::rotate_api_key))
        // Records
        .route(R::records::EXPORT.path, get(routes::records::records_export))
        .route(R::records::COUNT.path,  get(routes::records::records_count))
        .route(R::records::PRUNE.path,  delete(routes::records::records_prune))
        // Monitor
        .route(R::monitor::STATUS.path,        get(routes::monitor::monitor_status))
        .route(R::monitor::METRICS.path,       get(routes::monitor::monitor_metrics))
        .route(R::monitor::ALERTS_LIST.path,   get(routes::monitor::monitor_alerts_list).post(routes::monitor::monitor_alert_create))
        .route(R::monitor::ALERTS_DELETE.path, delete(routes::monitor::monitor_alert_delete))
        // Events
        .route(R::events::PUBLISH.path,              post(routes::events::publish_event))
        .route(R::events::SUBSCRIPTIONS_LIST.path,   get(routes::events::list_subscriptions).post(routes::events::create_subscription))
        .route(R::events::SUBSCRIPTIONS_DELETE.path, delete(routes::events::delete_subscription))
        // Queue management
        .route(R::queues::LIST.path,            get(routes::queue_mgmt::list_queues).post(routes::queue_mgmt::create_queue))
        .route(R::queues::GET.path,             get(routes::queue_mgmt::get_queue).delete(routes::queue_mgmt::delete_queue))
        .route(R::queues::PUBLISH.path,         post(routes::queue_mgmt::publish_message))
        .route(R::queues::BINDINGS_LIST.path,   get(routes::queue_mgmt::list_bindings).post(routes::queue_mgmt::create_binding))
        .route(R::queues::PURGE.path,           post(routes::queue_mgmt::purge_queue))
        .route(R::queues::DLQ_LIST.path,        get(routes::queue_mgmt::list_dlq))
        .route(R::queues::DLQ_REPLAY.path,      post(routes::queue_mgmt::replay_dlq))
        // Schedules
        .route(R::schedules::LIST.path,    get(routes::schedules::list_schedules).post(routes::schedules::create_schedule))
        .route(R::schedules::DELETE.path,  delete(routes::schedules::delete_schedule))
        .route(R::schedules::PAUSE.path,   post(routes::schedules::pause_schedule))
        .route(R::schedules::RESUME.path,  post(routes::schedules::resume_schedule))
        .route(R::schedules::RUN.path,     post(routes::schedules::run_schedule_now))
        .route(R::schedules::HISTORY.path, get(routes::schedules::schedule_history))
        // Environments
        .route(R::environments::LIST.path,   get(routes::environments::list_environments).post(routes::environments::create_environment))
        .route(R::environments::CLONE.path,  post(routes::environments::clone_environment))
        .route(R::environments::DELETE.path, delete(routes::environments::delete_environment))
        // Routes (gateway config)
        .route(R::config::LIST.path, get(routes::gateway_config::list_routes))
        .route(R::config::SYNC.path, post(routes::gateway_config::sync_routes))
        // SSE live streams
        .route(R::stream::EVENTS.path,     get(routes::stream::stream_events))
        .route(R::stream::EXECUTIONS.path, get(routes::stream::stream_executions))
        .route(R::stream::MUTATIONS.path,  get(routes::stream::stream_mutations))
        // Auth middleware injects RequestContext
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::auth::require_auth,
        ));

    // ── Auth: public routes (no require_auth guard) ───────────────────────
    let auth_router = Router::new()
        .route(R::auth::STATUS.path,       get(auth::routes::status))
        .route(R::auth::SETUP.path,        post(auth::routes::setup))
        .route(R::auth::LOGIN.path,        post(auth::routes::login))
        .route(R::auth::LOGOUT.path,       post(auth::routes::logout))
        .route(R::auth::ME.path,           get(auth::routes::me))
        .route(R::auth::USERS_LIST.path,   get(auth::routes::list_users).post(auth::routes::create_user))
        .route(R::auth::USERS_DELETE.path, delete(auth::routes::delete_user));

    Router::new()
        .merge(api)
        .merge(auth_router)
        .nest("/internal", internal)
        // ── Execution-plane guard ──────────────────────────────────────────
        .route(R::execution::RUN.path,            any(routes::system::execution_not_allowed))
        .route(R::execution::RUN_WILDCARD.path,   any(routes::system::execution_not_allowed))
        .route(R::execution::INVOKE.path,         any(routes::system::execution_not_allowed))
        .route(R::execution::INVOKE_WILDCARD.path, any(routes::system::execution_not_allowed))
        .route(R::execution::EXECUTE.path,        any(routes::system::execution_not_allowed))
        .route(R::execution::EXECUTE_WILDCARD.path, any(routes::system::execution_not_allowed))
        .route(R::execution::FUNCTION_RUN.path,   any(routes::system::execution_not_allowed))
        .route(R::execution::FUNCTION_INVOKE.path, any(routes::system::execution_not_allowed))
        // ── Swagger UI ────────────────────────────────────────────────────
        .route(R::sdk::OPENAPI_UI.path,        get(routes::openapi::ui))
        // ── Utility ───────────────────────────────────────────────────────
        .route(R::health::HEALTH.path, get(|| async { Json(serde_json::json!({ "status": "ok" })) }))
        .route(R::health::VERSION.path, get(|| async {
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


