//! Axum route table.
//!
//! Three routes — exactly matching `docs/gateway.md`:
//!   GET /health    — liveness probe (always 200)
//!   GET /readiness — readiness probe (503 until snapshot loaded)
//!   ANY /{*path}   — function invocation
use axum::{middleware, routing::{any, get}, Router};
use tower_http::cors::{CorsLayer, Any};
use crate::state::SharedState;

pub fn create_router(state: SharedState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health",    get(crate::handlers::health::handle))
        .route("/readiness", get(crate::handlers::readiness::handle))
        .route("/{*path}",   any(crate::handlers::dispatch::handle))
        .layer(middleware::from_fn_with_state(state.clone(), crate::metrics::record_metrics))
        .layer(cors)
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
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
        // Test that OPTIONS request handles cors headers properly.
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

        // 200 OK because tower-http cors intercepts OPTIONS requests to any mapped route.
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin").unwrap(),
            "*"
        );
        let methods = response.headers().get("access-control-allow-methods").unwrap();
        // Since Any allows all methods, it replies with access-control-allow-methods: *
        assert_eq!(methods, "*");
    }
}
