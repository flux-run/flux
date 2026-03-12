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