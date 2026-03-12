//! `GET /readiness` — 200 once the route snapshot is loaded, 503 until then.
//!
//! Kubernetes readiness probes gate traffic on this.
//! `SKIP_SNAPSHOT_READY_CHECK=1` disables the 503 for local dev.
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use crate::state::SharedState;

pub async fn handle(State(state): State<SharedState>) -> impl IntoResponse {
    let data = state.snapshot.get_data().await;
    let ready = !data.routes.is_empty()
        || std::env::var("SKIP_SNAPSHOT_READY_CHECK").is_ok();

    if ready {
        (StatusCode::OK, Json(serde_json::json!({
            "status": "ready",
            "routes": data.routes.len(),
        }))).into_response()
    } else {
        (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
            "status":  "loading",
            "message": "Route snapshot not yet loaded — retry in a moment",
        }))).into_response()
    }
}
