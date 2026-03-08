use std::sync::Arc;
use axum::{Router, routing::{post, get}};
use crate::state::AppState;

pub fn routes(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/jobs", post(crate::api::handlers::create_job::handler))
        .route("/jobs/stats", get(crate::api::handlers::stats::handler))
        .route(
            "/jobs/:id",
            get(crate::api::handlers::get_job::handler)
                .delete(crate::api::handlers::cancel_job::handler),
        )
        .route("/jobs/:id/retry", post(crate::api::handlers::retry_job::handler))
        .route("/health", get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }))
        .with_state(state)
}