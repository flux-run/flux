use std::sync::Arc;
use axum::{Router, routing::{post, get}};
use crate::state::AppState;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/jobs",
            get(crate::api::handlers::list_jobs::handler)
                .post(crate::api::handlers::create_job::handler),
        )
        .route("/jobs/stats", get(crate::api::handlers::stats::handler))
        .route(
            "/jobs/{id}",
            get(crate::api::handlers::get_job::handler)
                .delete(crate::api::handlers::cancel_job::handler),
        )
        .route("/jobs/{id}/retry", post(crate::api::handlers::retry_job::handler))
        .route("/health", get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }))
        .route("/version", get(|| async {
            axum::Json(serde_json::json!({
                "service": "queue",
                "commit": std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
                "build_time": std::env::var("BUILD_TIME").unwrap_or_else(|_| "unknown".to_string())
            }))
        }))
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

        async fn get_secrets(
            &self,
            _project_id: Option<Uuid>,
        ) -> Result<HashMap<String, String>, String> {
            Ok(HashMap::new())
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
}
