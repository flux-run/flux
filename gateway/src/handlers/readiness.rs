//! `GET /readiness` — 200 once the route snapshot is loaded AND the DB is reachable.
//!
//! Kubernetes readiness probes gate traffic on this.
//! `SKIP_SNAPSHOT_READY_CHECK=1` disables the 503 for local dev.
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use crate::state::SharedState;

pub async fn handle(State(state): State<SharedState>) -> impl IntoResponse {
    // 1. Database connectivity check — a failed DB means auth, trace writes and
    //    rate-limit updates will all fail, so we must not accept traffic.
    let db_ok = sqlx::query("SELECT 1")
        .execute(&state.db_pool)
        .await
        .is_ok();

    // 2. Route snapshot check — an empty snapshot means we cannot route anything.
    let data = state.snapshot.get_data().await;
    let routes_ok = !data.routes.is_empty()
        || std::env::var("SKIP_SNAPSHOT_READY_CHECK").is_ok();

    if routes_ok && db_ok {
        (StatusCode::OK, Json(serde_json::json!({
            "status": "ready",
            "routes": data.routes.len(),
            "db":     "ok",
        }))).into_response()
    } else {
        tracing::warn!(
            db_ok   = db_ok,
            routes  = data.routes.len(),
            "readiness check failed"
        );
        (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({
            "status":  "not_ready",
            "routes":  if routes_ok { "ok" } else { "loading" },
            "db":      if db_ok { "ok" } else { "error" },
            "message": "Service not ready",
        }))).into_response()
    }
}
