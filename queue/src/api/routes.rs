use axum::{Router, routing::{post, get}};
use sqlx::PgPool;

pub fn routes(pool: PgPool) -> Router {
    Router::new()
        .route("/jobs", post(crate::api::handlers::create_job::handler))
        .route(
            "/jobs/:id",
            get(crate::api::handlers::get_job::handler)
                .delete(crate::api::handlers::cancel_job::handler),
        )
        .route("/jobs/:id/retry", post(crate::api::handlers::retry_job::handler))
        .route("/health", get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }))
        .with_state(pool)
}