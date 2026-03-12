//! `GET /health` — always returns 200.
//!
//! Used by load-balancers and Kubernetes liveness probes.
//! Does NOT check dependencies — use `/readiness` for that.
use axum::Json;

pub async fn handle() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}
