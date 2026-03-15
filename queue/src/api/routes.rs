use std::sync::Arc;
use axum::{Router, middleware, routing::{post, get}};
use api_contract::routes as R;
use crate::state::AppState;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route(R::jobs::LIST.path,
            get(crate::api::handlers::list_jobs::handler)
                .post(crate::api::handlers::create_job::handler),
        )
        .route(R::jobs::STATS.path,  get(crate::api::handlers::stats::handler))
        .route(
            R::jobs::GET.path,
            get(crate::api::handlers::get_job::handler)
                .delete(crate::api::handlers::cancel_job::handler),
        )
        .route(R::jobs::RETRY.path,   post(crate::api::handlers::retry_job::handler))
        .route(R::health::HEALTH.path, get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }))
        .route(R::health::VERSION.path, get(|| async {
            axum::Json(serde_json::json!({
                "service": "queue",
                "commit": std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
                "build_time": std::env::var("BUILD_TIME").unwrap_or_else(|_| "unknown".to_string())
            }))
        }))
        .layer(middleware::from_fn(crate::api::middleware::require_service_token))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::{
        body::{to_bytes, Body},
        http::{Request, StatusCode},
        Router,
    };
    use job_contract::dispatch::ApiDispatch;
    use serde_json::Value;
    use sqlx::postgres::PgPoolOptions;
    use tower::util::ServiceExt;
    use uuid::Uuid;

    use super::routes;
    use crate::state::AppState;

    struct MockApiDispatch;

    #[async_trait]
    impl ApiDispatch for MockApiDispatch {
        async fn get_bundle(&self, _function_id: &str) -> Result<Value, String> {
            Ok(serde_json::json!({}))
        }

        async fn write_log(&self, _entry: Value) -> Result<(), String> {
            Ok(())
        }

        async fn get_secrets(&self) -> Result<HashMap<String, String>, String> {
            Ok(HashMap::new())
        }

        async fn resolve_function(&self, _name: &str) -> Result<job_contract::dispatch::ResolvedFunction, String> {
            Err("not implemented".to_string())
        }
    }

    fn app() -> Router {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/flux")
            .unwrap();
        let state = Arc::new(AppState::new(pool, Arc::new(MockApiDispatch)));
        routes(state)
    }

    #[tokio::test]
    async fn health_route_returns_ok() {
        let response = app()
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
        let response = app()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Expect 405 Method Not Allowed for POST /health
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn version_route_returns_ok() {
        std::env::set_var("GIT_SHA", "testhash123");
        std::env::set_var("BUILD_TIME", "1970");

        let response = app()
            .oneshot(Request::builder().uri("/version").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["service"], "queue");
        assert_eq!(json["commit"], "testhash123");
        assert_eq!(json["build_time"], "1970");

        // Clean up
        std::env::remove_var("GIT_SHA");
        std::env::remove_var("BUILD_TIME");
    }
}
