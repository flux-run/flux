//! Axum route table.
//!
//! Three routes — exactly matching `docs/gateway.md`:
//!   GET /health    — liveness probe (always 200)
//!   GET /readiness — readiness probe (503 until snapshot loaded)
//!   ANY /{*path}   — function invocation
use axum::{middleware, routing::{any, get}, Router};
use tower_http::cors::{AllowOrigin, CorsLayer};
use api_contract::routes as R;
use crate::state::SharedState;

/// Build the CORS layer.
///
/// In production (`FLUX_ENV=production`), `CORS_ALLOWED_ORIGINS` **must** be
/// set to a comma-separated list of allowed origins, e.g.:
///
///   CORS_ALLOWED_ORIGINS=https://app.example.com,https://admin.example.com
///
/// In development (default), origins are unrestricted so local tooling works
/// without configuration, and a warning is logged.
fn build_cors() -> CorsLayer {
    use tower_http::cors::Any;

    let is_production = std::env::var("FLUX_ENV").as_deref() == Ok("production");
    let configured = std::env::var("CORS_ALLOWED_ORIGINS").ok();

    match configured {
        Some(origins_str) => {
            let origins: Vec<axum::http::HeaderValue> = origins_str
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse().ok())
                .collect();

            if origins.is_empty() {
                if is_production {
                    panic!("[Flux] CORS_ALLOWED_ORIGINS is set but contains no valid origins. \
                            Cannot start in production with no allowed CORS origins.");
                }
                tracing::warn!("[Flux] CORS_ALLOWED_ORIGINS is empty or invalid — falling back to allow-all in dev mode.");
                return CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);
            }

            CorsLayer::new()
                .allow_origin(AllowOrigin::list(origins))
                .allow_methods(Any)
                .allow_headers(Any)
        }
        None => {
            if is_production {
                panic!("[Flux] CORS_ALLOWED_ORIGINS must be set in production. \
                        Example: CORS_ALLOWED_ORIGINS=https://your-app.example.com");
            }
            tracing::warn!(
                "[Flux] CORS_ALLOWED_ORIGINS not set — allowing all origins in dev mode. \
                 Set CORS_ALLOWED_ORIGINS in production."
            );
            CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any)
        }
    }
}

pub fn create_router(state: SharedState) -> Router {
    Router::new()
        .route(R::health::HEALTH.path,        get(crate::handlers::health::handle))
        .route(R::health::READINESS.path,     get(crate::handlers::readiness::handle))
        .route(R::internal::METRICS.path,     get(crate::metrics::prometheus_handler))
        .route(R::proxy::GATEWAY_DISPATCH.path, any(crate::handlers::dispatch::handle))
        .layer(middleware::from_fn_with_state(state.clone(), crate::metrics::record_metrics))
        .layer(build_cors())
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;

    // Serializes tests that read/write CORS env vars to prevent races between
    // cors_headers_on_health_checks and cors_restricted_to_configured_origin.
    static CORS_ENV_LOCK: Mutex<()> = Mutex::new(());
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
    };
    use job_contract::dispatch::{ExecuteRequest, ExecuteResponse, RuntimeDispatch};
    use sqlx::postgres::PgPoolOptions;
    use tower::util::ServiceExt;

    use super::create_router;
    use crate::{auth::JwksCache, snapshot::GatewaySnapshot, GatewayState};

    struct MockRuntimeDispatch;

    #[async_trait]
    impl RuntimeDispatch for MockRuntimeDispatch {
        async fn execute(&self, _req: ExecuteRequest) -> Result<ExecuteResponse, String> {
            Ok(ExecuteResponse {
                body: serde_json::json!({ "ok": true }),
                status: 200,
                duration_ms: 1,
            })
        }
    }

    fn state() -> Arc<GatewayState> {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/flux")
            .unwrap();

        Arc::new(GatewayState {
            db_pool: pool.clone(),
            runtime: Arc::new(MockRuntimeDispatch),
            snapshot: GatewaySnapshot::new(pool, "postgres://postgres:postgres@localhost/flux".into()),
            jwks_cache: JwksCache::new(reqwest::Client::new()),
            max_request_size_bytes: 1024 * 1024,
            rate_limit_per_sec: 50,
            local_mode: true,
        })
    }

    #[tokio::test]
    async fn health_route_returns_ok() {
        let response = create_router(state())
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
    }

    #[tokio::test]
    async fn health_route_wrong_method() {
        let response = create_router(state())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Axum routes matched by exact paths prioritize methods. Since only GET is registered for /health,
        // it returns a 405 Method Not Allowed instead of falling back to `/{*path}`
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn cors_headers_on_health_checks() {
        // Serialize against cors_restricted_to_configured_origin to avoid env-var races.
        let _lock = CORS_ENV_LOCK.lock().unwrap();
        // In dev mode (no CORS_ALLOWED_ORIGINS), the gateway allows all origins.
        unsafe { std::env::remove_var("CORS_ALLOWED_ORIGINS"); }
        unsafe { std::env::remove_var("FLUX_ENV"); }

        let response = create_router(state())
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/health")
                    .header("Origin", "https://example.com")
                    .header("Access-Control-Request-Method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin").unwrap(),
            "*"
        );
        let methods = response.headers().get("access-control-allow-methods").unwrap();
        assert_eq!(methods, "*");
    }

    #[tokio::test]
    async fn cors_restricted_to_configured_origin() {
        // Serialize against cors_headers_on_health_checks to avoid env-var races.
        let _lock = CORS_ENV_LOCK.lock().unwrap();
        unsafe {
            std::env::remove_var("FLUX_ENV");
            std::env::set_var("CORS_ALLOWED_ORIGINS", "https://app.example.com");
        }

        let response = create_router(state())
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/health")
                    .header("Origin", "https://app.example.com")
                    .header("Access-Control-Request-Method", "GET")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let allow_origin = response
            .headers()
            .get("access-control-allow-origin")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        assert_eq!(allow_origin, "https://app.example.com");

        unsafe { std::env::remove_var("CORS_ALLOWED_ORIGINS"); }
    }
}
