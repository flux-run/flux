//! Stub handlers for features that are planned but not yet fully implemented.
//!
//! These routes exist so the CLI never receives a 404 for a known path.
//! Lists return empty data; mutations return HTTP 501 with a clear message.
//! When a feature graduates to production, replace the stub with a real handler.

use axum::{
    extract::Path,
    http::StatusCode,
    Json,
};
use serde_json::{json, Value};
use uuid::Uuid;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn not_impl(feature: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error":   "not_implemented",
            "message": format!("{} is not yet enabled on this server", feature),
            "code":    501u16,
        })),
    )
}

fn empty_list() -> Json<Value> {
    Json(json!({ "data": [], "count": 0 }))
}

fn deleted() -> StatusCode {
    StatusCode::NO_CONTENT
}

// ── API Keys ──────────────────────────────────────────────────────────────────

pub async fn api_keys_list() -> Json<Value> {
    empty_list()
}

pub async fn api_key_create() -> (StatusCode, Json<Value>) {
    let id  = Uuid::new_v4();
    let key = format!("flux_{}", Uuid::new_v4().to_string().replace('-', ""));
    (
        StatusCode::CREATED,
        Json(json!({
            "data": {
                "id":         id,
                "key":        key,
                "created_at": chrono::Utc::now().to_rfc3339(),
            }
        })),
    )
}

pub async fn api_key_delete(Path(_id): Path<String>) -> StatusCode {
    deleted()
}

pub async fn api_key_rotate(Path(_id): Path<String>) -> (StatusCode, Json<Value>) {
    let key = format!("flux_{}", Uuid::new_v4().to_string().replace('-', ""));
    (
        StatusCode::OK,
        Json(json!({ "data": { "key": key } })),
    )
}

// ── Monitor ───────────────────────────────────────────────────────────────────

pub async fn monitor_status() -> Json<Value> {
    Json(json!({
        "status": "ok",
        "uptime_seconds": 0,
        "services": {
            "api":        { "status": "ok" },
            "gateway":    { "status": "ok" },
            "runtime":    { "status": "ok" },
            "data_engine":{ "status": "ok" },
            "queue":      { "status": "ok" },
        }
    }))
}

pub async fn monitor_metrics() -> Json<Value> {
    Json(json!({
        "data": {
            "requests_total": 0,
            "errors_total":   0,
            "p50_ms":         0,
            "p95_ms":         0,
            "p99_ms":         0,
        },
        "window": "1h"
    }))
}

pub async fn monitor_alerts_list() -> Json<Value> {
    empty_list()
}

pub async fn monitor_alert_create() -> (StatusCode, Json<Value>) {
    not_impl("Monitor alerts")
}

pub async fn monitor_alert_delete(Path(_id): Path<String>) -> StatusCode {
    deleted()
}

// ── Events ────────────────────────────────────────────────────────────────────

pub async fn events_publish() -> (StatusCode, Json<Value>) {
    not_impl("Event publishing")
}

pub async fn events_subscriptions_list() -> Json<Value> {
    empty_list()
}

pub async fn events_subscribe() -> (StatusCode, Json<Value>) {
    not_impl("Event subscriptions")
}

pub async fn events_unsubscribe(Path(_id): Path<String>) -> StatusCode {
    deleted()
}

// ── Queue management API ──────────────────────────────────────────────────────

pub async fn queues_list() -> Json<Value> {
    empty_list()
}

pub async fn queue_create() -> (StatusCode, Json<Value>) {
    not_impl("Queue management")
}

pub async fn queue_get(Path(_name): Path<String>) -> Json<Value> {
    Json(json!({ "data": null }))
}

pub async fn queue_delete(Path(_name): Path<String>) -> StatusCode {
    deleted()
}

pub async fn queue_publish_message(Path(_name): Path<String>) -> (StatusCode, Json<Value>) {
    not_impl("Queue message publishing")
}

pub async fn queue_bindings_list(Path(_name): Path<String>) -> Json<Value> {
    empty_list()
}

pub async fn queue_binding_create(Path(_name): Path<String>) -> (StatusCode, Json<Value>) {
    not_impl("Queue bindings")
}

pub async fn queue_purge(Path(_name): Path<String>) -> (StatusCode, Json<Value>) {
    not_impl("Queue purge")
}

pub async fn queue_dlq_list(Path(_name): Path<String>) -> Json<Value> {
    empty_list()
}

pub async fn queue_dlq_replay(Path(_name): Path<String>) -> (StatusCode, Json<Value>) {
    not_impl("DLQ replay")
}

// ── Schedules ─────────────────────────────────────────────────────────────────

pub async fn schedules_list() -> Json<Value> {
    empty_list()
}

pub async fn schedule_create() -> (StatusCode, Json<Value>) {
    not_impl("Schedules")
}

pub async fn schedule_delete(Path(_name): Path<String>) -> StatusCode {
    deleted()
}

pub async fn schedule_pause(Path(_name): Path<String>) -> (StatusCode, Json<Value>) {
    not_impl("Schedules")
}

pub async fn schedule_resume(Path(_name): Path<String>) -> (StatusCode, Json<Value>) {
    not_impl("Schedules")
}

pub async fn schedule_run_now(Path(_name): Path<String>) -> (StatusCode, Json<Value>) {
    not_impl("Schedules")
}

pub async fn schedule_history(Path(_name): Path<String>) -> Json<Value> {
    empty_list()
}

// ── Agents ────────────────────────────────────────────────────────────────────

pub async fn agents_list() -> Json<Value> {
    empty_list()
}

pub async fn agent_create() -> (StatusCode, Json<Value>) {
    not_impl("Agents")
}

pub async fn agent_get(Path(_name): Path<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "not_found", "message": "Agent not found" })),
    )
}

pub async fn agent_delete(Path(_name): Path<String>) -> StatusCode {
    deleted()
}

pub async fn agent_run(Path(_name): Path<String>) -> (StatusCode, Json<Value>) {
    not_impl("Agents")
}

pub async fn agent_simulate(Path(_name): Path<String>) -> (StatusCode, Json<Value>) {
    not_impl("Agents")
}

// ── Environments ──────────────────────────────────────────────────────────────

pub async fn environments_list() -> Json<Value> {
    Json(json!({
        "data": [
            { "name": "production",  "slug": "production",  "default": true  },
            { "name": "development", "slug": "development",  "default": false },
        ],
        "count": 2
    }))
}

pub async fn environment_create() -> (StatusCode, Json<Value>) {
    not_impl("Environment management")
}

pub async fn environment_delete(Path(_name): Path<String>) -> StatusCode {
    deleted()
}

pub async fn environments_clone() -> (StatusCode, Json<Value>) {
    not_impl("Environment cloning")
}

// ── Gateway extras ────────────────────────────────────────────────────────────

/// GET /gateway/routes/{id} — get a single route by ID.
pub async fn get_gateway_route_by_id(Path(_id): Path<String>) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({ "error": "not_found", "message": "Route not found" })),
    )
}

/// POST /gateway/middleware — attach middleware to a route.
pub async fn gateway_middleware_create() -> (StatusCode, Json<Value>) {
    not_impl("Gateway middleware management")
}

/// DELETE /gateway/middleware/{route}/{type} — remove middleware.
pub async fn gateway_middleware_delete(
    Path((_route, _middleware_type)): Path<(String, String)>,
) -> StatusCode {
    deleted()
}

/// PUT /gateway/routes/{id}/rate-limit — set rate limit on a route.
pub async fn gateway_route_rate_limit_set(Path(_id): Path<String>) -> (StatusCode, Json<Value>) {
    not_impl("Per-route rate limiting")
}

/// DELETE /gateway/routes/{id}/rate-limit — remove rate limit.
pub async fn gateway_route_rate_limit_delete(Path(_id): Path<String>) -> StatusCode {
    deleted()
}

/// PUT /gateway/routes/{id}/cors — set CORS policy on a route.
pub async fn gateway_route_cors_set(Path(_id): Path<String>) -> (StatusCode, Json<Value>) {
    not_impl("Per-route CORS configuration")
}

/// GET /gateway/routes/{id}/cors — get CORS policy.
pub async fn gateway_route_cors_get(Path(_id): Path<String>) -> Json<Value> {
    Json(json!({ "data": null }))
}
