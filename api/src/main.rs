mod config;
mod db;
mod middleware;
mod models;
mod routes;
mod services;
mod types;
mod secrets;
mod api_keys;
mod logs;

use axum::{
    middleware as axum_middleware,
    routing::{any, delete, get, post, put},
    Json,
    Router,
};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tracing::info;
use types::scope::Scope;
use firebase_auth::FirebaseAuth;
use std::sync::Arc;
use tower_http::cors::{CorsLayer, AllowOrigin};
use axum::http::{HeaderValue, Method, header};

/// Capacity of the in-process event broadcast channel.
/// Lagging receivers simply skip missed messages.
const EVENT_CHANNEL_CAPACITY: usize = 1_024;

#[derive(Clone)]
pub struct AppState {
    pub pool: sqlx::PgPool,
    pub firebase_auth: Arc<FirebaseAuth>,
    pub storage: services::storage::StorageService,
    /// Bucket names for each storage tier.  Loaded once at startup from env.
    pub storage_config: services::storage::StorageConfig,
    pub http_client: reqwest::Client,
    pub data_engine_url: String,
    /// Gateway URL forwarded to OpenAPI spec servers[].
    pub gateway_url: String,
    /// In-memory SDK generation cache.
    /// Key:   "{project_id}:{schema_hash}"
    /// Value: generated TypeScript source
    /// Invalidated automatically when the schema changes (new hash ≠ cached key).
    pub sdk_cache: Arc<tokio::sync::RwLock<std::collections::HashMap<String, String>>>,
    /// Broadcast channel for real-time table-change events.
    /// Each message is a JSON string:
    ///   `{"project_id":"...","table":"users","operation":"insert","row":{...}}`
    /// SSE handlers subscribe to this and filter by project_id.
    pub event_tx: tokio::sync::broadcast::Sender<String>,
    /// Background log archiver — moves logs older than `LOG_HOT_DAYS` to R2/S3.
    /// Also used by the read path to fetch archive data for old `since` queries.
    pub log_archiver: std::sync::Arc<logs::archiver::LogArchiver>,
}

impl axum::extract::FromRef<AppState> for sqlx::PgPool {
    fn from_ref(state: &AppState) -> Self {
        state.pool.clone()
    }
}

impl axum::extract::FromRef<AppState> for Arc<FirebaseAuth> {
    fn from_ref(state: &AppState) -> Self {
        state.firebase_auth.clone()
    }
}

/// Build the CORS layer from the `ALLOWED_ORIGINS` environment variable.
///
/// `ALLOWED_ORIGINS` is a comma-separated list of allowed origins, e.g.:
///   `http://localhost:5173,https://fluxbase.co`
///
/// If the variable is not set, defaults to `http://localhost:5173`.
pub fn build_cors() -> CorsLayer {
    let raw = std::env::var("ALLOWED_ORIGINS")
        .unwrap_or_else(|_| "http://localhost:5173".to_string());

    let origins: Vec<HeaderValue> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse::<HeaderValue>().expect("Invalid ALLOWED_ORIGINS entry"))
        .collect();

    info!("CORS allowed origins: {:?}", origins);

    let origins_list = origins.clone();

    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _parts| {
            let Ok(origin_str) = origin.to_str() else { return false };
            let origin_lc = origin_str.to_lowercase();
            
            if origins_list.iter().any(|o| o == origin) {
                return true;
            }

            // More inclusive check for production domains
            let allowed = origin_lc.ends_with(".fluxbase.co") 
                || origin_lc == "https://fluxbase.co" 
                || origin_lc == "http://fluxbase.co";
            
            if !allowed {
                tracing::warn!("CORS origin denied: {}", origin_str);
            }
            allowed
        }))
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
            "x-fluxbase-tenant".parse().unwrap(),
            "x-fluxbase-project".parse().unwrap(),
        ])
        .allow_credentials(true)
        .max_age(std::time::Duration::from_secs(3600))
}

pub fn create_app(state: AppState) -> Router {
    // Platform Scope routes (just need valid Firebase token)
    let platform_routes = Router::new()
        .route("/auth/me", get(routes::auth::get_me))
        .route("/auth/logout", post(routes::auth::logout))
        .route("/platform/runtimes", get(routes::platform::list_runtimes))
        .route("/platform/services", get(routes::platform::list_services))
        .route("/tenants", post(routes::tenants::create_tenant))
        .route("/tenants", get(routes::tenants::get_tenants))
        .route("/tenants/{id}", get(routes::tenants::get_tenant))
        .route("/tenants/{id}", delete(routes::tenants::delete_tenant))
        .layer(axum_middleware::from_fn(|req, next| {
            middleware::scope::require_scope(Scope::Platform, req, next)
        }));

    let internal_routes = Router::new()
        .route("/secrets", get(secrets::routes::get_internal_runtime_secrets))
        .route("/bundle", get(routes::deployments::get_internal_bundle))
        .route("/logs", post(logs::routes::create_log))
        .route("/logs", get(logs::routes::list_logs))
        // Emits a table-change event to all connected SSE clients for the project.
        .route("/events/emit", post(routes::events::emit));

    // Tenant Scope routes (X-Fluxbase-Tenant required + membership verified)
    let tenant_routes = Router::new()
        .route("/tenants/{id}/members", get(routes::tenants::get_members))
        .route("/tenants/{id}/members", post(routes::tenants::invite_member))
        .route("/tenants/{id}/members/{user}", delete(routes::tenants::remove_member))
        .route("/projects", get(routes::projects::get_projects))
        .route("/projects", post(routes::projects::create_project))
        .route("/projects/{id}", get(routes::projects::get_project))
        .route("/projects/{id}", delete(routes::projects::delete_project))
        .layer(axum_middleware::from_fn(|req, next| {
            middleware::scope::require_scope(Scope::Tenant, req, next)
        }));

    // Project Scope routes (X-Fluxbase-Project required + project verified under tenant)
    let project_routes = Router::new()
        .route("/api-keys", get(crate::api_keys::routes::list_api_keys))
        .route("/api-keys", post(crate::api_keys::routes::create_api_key))
        .route("/secrets", get(secrets::routes::list_secrets))
        .route("/secrets", post(secrets::routes::create_secret))
        .route("/secrets/{key}", put(secrets::routes::update_secret))
        .route("/secrets/{key}", delete(secrets::routes::delete_secret))
        .route("/functions", get(routes::functions::list_functions))
        .route("/functions", post(routes::functions::create_function))
        .route("/functions/{id}", get(routes::functions::get_function))
        .route("/functions/{id}", delete(routes::functions::delete_function))
        .route("/functions/deploy", post(routes::deployments::deploy_function_cli))
        .route("/deployments", post(routes::deployments::create_deployment))
        .route("/deployments/list/{id}", get(routes::deployments::list_deployments))
        .route("/deployments/{id}/activate/{version}", post(routes::deployments::activate_deployment))
        // Gateway Routes
        .route("/routes", get(routes::gateway_routes::list_gateway_routes).post(routes::gateway_routes::create_gateway_route))
        .route("/routes/{id}", axum::routing::patch(routes::gateway_routes::update_gateway_route).delete(routes::gateway_routes::delete_gateway_route))
        // Schema graph — unified table + function metadata for code generation.
        .route("/schema/graph",    get(routes::schema::graph))
        // SDK endpoints — raw schema graph + on-demand TypeScript SDK generation.
        .route("/sdk/schema",      get(routes::sdk::schema))
        .route("/sdk/typescript",  get(routes::sdk::typescript))
        // OpenAPI 3.0 spec — generated from live schema.
        .route("/openapi.json",    get(routes::openapi::spec))
        // Realtime SSE — subscribe to table-change events.
        .route("/events/stream",   get(routes::events::stream))
        // Function logs — project-scoped, supports ?function=&limit=&since=
        .route("/logs",            get(logs::routes::list_project_logs))
        // Full request trace — all log spans for a given request_id across services
        .route("/traces/{request_id}", get(logs::routes::get_trace))
        // Integrations + tool catalog
        .route("/tools",                     get(routes::tools::list_tools))
        .route("/tools/connected",           get(routes::tools::list_connected))
        .route("/tools/connect/{provider}",  post(routes::tools::connect_provider))
        .route("/tools/disconnect/{provider}", delete(routes::tools::disconnect_provider))
        // Data Engine management proxy — CRUD for databases, tables, schema,
        // relationships, policies, hooks, subscriptions, workflows, cron.
        // Execution traffic (POST /db/query) is routed to the gateway instead.
        .route("/db/{*path}",    any(routes::data_engine::proxy_handler))
        .route("/files/{*path}", any(routes::data_engine::proxy_handler))
        .layer(axum_middleware::from_fn(|req, next| {
            middleware::scope::require_scope(Scope::Project, req, next)
        }));

    // Combine with core authentication middleware applied to all.
    // CORS is outermost so preflight OPTIONS requests are handled before auth.
    let mixed_tenant_project_routes = Router::new()
        .route("/api-keys/{id}", delete(crate::api_keys::routes::revoke_api_key))
        .layer(axum_middleware::from_fn(|req, next| {
            middleware::scope::require_scope(Scope::Tenant, req, next)
        }));

    // Combine authenticated routes
    let authenticated_api = Router::new()
        .merge(platform_routes)
        .merge(tenant_routes)
        .merge(project_routes)
        .merge(mixed_tenant_project_routes)
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::context::resolve_context,
        ))
        .layer(axum_middleware::from_fn_with_state(
            state.clone(),
            middleware::auth::verify_auth,
        ));

    // Combine all
    Router::new()
        .merge(authenticated_api)
        .nest("/internal", internal_routes)
        // ── Execution-plane guard ──────────────────────────────────────────
        // The API service is the CONTROL PLANE only.  All runtime execution
        // traffic must flow through {tenant_slug}.fluxbase.co (Gateway).
        // These routes explicitly reject any attempt to invoke function
        // execution here, so architectural drift fails loudly at runtime.
        .route("/run",                          any(routes::system::execution_not_allowed))
        .route("/run/{*path}",                  any(routes::system::execution_not_allowed))
        .route("/invoke",                       any(routes::system::execution_not_allowed))
        .route("/invoke/{*path}",               any(routes::system::execution_not_allowed))
        .route("/execute",                      any(routes::system::execution_not_allowed))
        .route("/execute/{*path}",              any(routes::system::execution_not_allowed))
        // Block SDK-generated execution patterns like /functions/{name}/run
        .route("/functions/{name}/run",         any(routes::system::execution_not_allowed))
        .route("/functions/{name}/invoke",      any(routes::system::execution_not_allowed))
        // ── Public demo endpoints (no auth, rate-limited by IP) ─────────────
        // Back the landing-page "Try Fluxbase" interactive trace demo.
        .route("/demo/signup",           post(routes::demo::demo_signup))
        .route("/demo/trace/{request_id}", get(routes::demo::demo_trace))
        // OAuth callback — Composio redirects here after provider OAuth; no auth required
        .route("/tools/oauth/callback", get(routes::tools::oauth_callback))
        .route("/health", get(|| async { Json(serde_json::json!({ "status": "ok" })) }))
        .route("/version", get(|| async {
            Json(serde_json::json!({
                "service": "api",
                "commit": std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
                "build_time": std::env::var("BUILD_TIME").unwrap_or_else(|_| "unknown".to_string())
            }))
        }))
        .fallback(|req: axum::extract::Request| async move {
            tracing::warn!("404 Route Not Found: {} {}", req.method(), req.uri().path());
            (axum::http::StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "not_found", "path": req.uri().path().to_string() })))
        })
        .layer(build_cors())
        // Outermost layer: assigns / propagates x-request-id and logs every
        // non-health request with method, path, status, and latency.
        .layer(axum_middleware::from_fn(middleware::request_id::request_id_middleware))
        .layer(axum::extract::DefaultBodyLimit::max(1 * 1024 * 1024)) // 1 MB
        .with_state(state)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    config::init();
    let pool = db::connection::init_pool().await?;
    let firebase_project_id = std::env::var("FIREBASE_PROJECT_ID").expect("FIREBASE_PROJECT_ID required");
    let firebase_auth = Arc::new(FirebaseAuth::new(&firebase_project_id).await);
    let storage        = services::storage::StorageService::new().await;
    let storage_config  = services::storage::StorageConfig::from_env();
    let log_archiver    = logs::archiver::LogArchiver::new(pool.clone()).await;
    log_archiver.clone().spawn_task();

    info!(
        "Storage buckets — files: {}, functions: {}, logs: {}",
        storage_config.files_bucket,
        storage_config.functions_bucket,
        storage_config.logs_bucket,
    );

    let (event_tx, _) = tokio::sync::broadcast::channel(EVENT_CHANNEL_CAPACITY);

    let state = AppState {
        pool,
        firebase_auth,
        storage,
        storage_config,
        http_client: reqwest::Client::new(),
        log_archiver,
        data_engine_url: std::env::var("DATA_ENGINE_URL")
            .unwrap_or_else(|_| "http://localhost:8082".to_string()),
        gateway_url: std::env::var("GATEWAY_URL")
            .unwrap_or_else(|_| "http://localhost:8081".to_string()),
        sdk_cache: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        event_tx,
    };
    
    let app = create_app(state);

    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .unwrap_or(8080);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    info!("Starting Fluxbase Control Plane on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{self, Request, StatusCode},
    };
    use tower::ServiceExt;
    use serde_json::Value;
    use std::sync::Once;

    static INIT: Once = Once::new();

    async fn setup_app() -> (Router, sqlx::PgPool) {
        INIT.call_once(|| {
            config::init();
        });
        let db_url = std::env::var("DATABASE_URL").expect("DATABASE_URL missing");
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect(&db_url)
            .await
            .unwrap();
            
        let firebase_auth = Arc::new(FirebaseAuth::new("mock-project").await);
        let storage = services::storage::StorageService::new().await;
        let state = AppState {
            pool: pool.clone(),
            firebase_auth,
            storage,
        };
        (create_app(state), pool)
    }

    async fn send_request(app: Router, req: Request<Body>) -> (StatusCode, Value) {
        let response = app.oneshot(req).await.unwrap();
        let status = response.status();
        let body_bytes = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let body_json = serde_json::from_slice(&body_bytes).unwrap_or(serde_json::json!({}));
        (status, body_json)
    }

    #[tokio::test]
    async fn test_1_login_platform_scope() {
        let (app, _) = setup_app().await;

        let req = Request::builder()
            .method(http::Method::GET)
            .uri("/auth/me")
            .header("Authorization", format!("Bearer {}", uuid::Uuid::new_v4()))
            .body(Body::empty())
            .unwrap();

        let (status, body) = send_request(app, req).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.get("user_id").is_some());
    }

    #[tokio::test]
    async fn test_2_create_tenant_and_verify_membership() {
        let (app, pool) = setup_app().await;

        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/tenants")
            .header("Authorization", format!("Bearer {}", uuid::Uuid::new_v4()))
            .header("Content-Type", "application/json")
            .body(Body::from("{\"name\":\"Test Tenant\"}"))
            .unwrap();

        let (status, body) = send_request(app, req).await;
        assert_eq!(status, StatusCode::CREATED);
        
        let tenant_id_str = body.get("tenant_id").unwrap().as_str().unwrap();
        let tenant_id = uuid::Uuid::parse_str(tenant_id_str).unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM tenant_members WHERE tenant_id = $1 AND role = 'owner'")
            .bind(tenant_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn test_3_create_project_under_tenant() {
        let (app, pool) = setup_app().await;
        let test_user_token = format!("Bearer {}", uuid::Uuid::new_v4());

        let req_tenant = Request::builder()
            .method(http::Method::POST)
            .uri("/tenants")
            .header("Authorization", &test_user_token)
            .header("Content-Type", "application/json")
            .body(Body::from("{\"name\":\"Test Corp\"}"))
            .unwrap();
        let (_, body) = send_request(app.clone(), req_tenant).await;
        let tenant_id = body.get("tenant_id").unwrap().as_str().unwrap().to_string();

        let req_proj = Request::builder()
            .method(http::Method::POST)
            .uri("/projects")
            .header("Authorization", &test_user_token)
            .header("X-Fluxbase-Tenant", &tenant_id)
            .header("Content-Type", "application/json")
            .body(Body::from("{\"name\":\"my-project\"}"))
            .unwrap();
        
        let (status, body) = send_request(app, req_proj).await;
        assert_eq!(status, StatusCode::CREATED);
        let project_id_str = body.get("project_id").unwrap().as_str().unwrap();
        let project_id = uuid::Uuid::parse_str(project_id_str).unwrap();

        let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM projects WHERE tenant_id = $1 AND id = $2")
            .bind(uuid::Uuid::parse_str(&tenant_id).unwrap())
            .bind(project_id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(count.0, 1);
    }

    #[tokio::test]
    async fn test_4_tenant_route_without_tenant_header_fails() {
        let (app, _) = setup_app().await;
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/projects")
            .header("Authorization", format!("Bearer {}", uuid::Uuid::new_v4()))
            .body(Body::empty())
            .unwrap();
        let (status, _) = send_request(app, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_5_project_route_without_project_header_fails() {
        let (app, _) = setup_app().await;
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/functions")
            .header("Authorization", format!("Bearer {}", uuid::Uuid::new_v4()))
            .header("X-Fluxbase-Tenant", uuid::Uuid::new_v4().to_string())
            .body(Body::empty())
            .unwrap();
        let (status, _) = send_request(app, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_6_project_accessed_from_different_tenant() {
        let (app, _) = setup_app().await;
        let req = Request::builder()
            .method(http::Method::POST)
            .uri("/functions")
            .header("Authorization", format!("Bearer {}", uuid::Uuid::new_v4()))
            .header("X-Fluxbase-Tenant", uuid::Uuid::new_v4().to_string())
            .header("X-Fluxbase-Project", uuid::Uuid::new_v4().to_string())
            .body(Body::empty())
            .unwrap();
        let (status, _) = send_request(app, req).await;
        assert_eq!(status, StatusCode::FORBIDDEN);
    }
}
