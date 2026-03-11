use axum::{middleware as axum_middleware, routing::{delete, get, patch, post}, Json, Router};
use serde_json::json;
use std::sync::Arc;

use crate::{
    api::{
        handlers::{cron, databases, debug, files, history, hooks, policies, query, relationships, schema, subscriptions, tables, workflows},
        middleware::service_auth::require_service_token,
    },
    state::AppState,
};

pub fn build(state: Arc<AppState>) -> Router {
    Router::new()
        // ── Data API ──────────────────────────────────────────────────────────
        .route("/db/query",                   post(query::handler))
        // ── Database management ───────────────────────────────────────────────
        .route("/db/databases",               post(databases::create).get(databases::list))
        .route("/db/databases/{name}",         delete(databases::drop_db))
        // ── Table management ──────────────────────────────────────────────────
        .route("/db/tables",                  post(tables::create))
        .route("/db/tables/{database}",        get(tables::list))
        .route("/db/tables/{database}/{table}", delete(tables::drop_table))
        // ── Policy management ─────────────────────────────────────────────────
        .route("/db/policies",               get(policies::list).post(policies::create))
        .route("/db/policies/{id}",           delete(policies::delete))
        // ── Hook management ───────────────────────────────────────────────────
        .route("/db/hooks",     get(hooks::list).post(hooks::create))
        .route("/db/hooks/{id}", patch(hooks::update).delete(hooks::delete))
        // ── Relationships ─────────────────────────────────────────────────────
        .route("/db/relationships",     get(relationships::list).post(relationships::create))
        .route("/db/relationships/{id}", delete(relationships::delete))
        // ── Event subscriptions ─────────────────────────────────────────────
        .route("/db/subscriptions",     get(subscriptions::list).post(subscriptions::create))
        .route("/db/subscriptions/{id}", patch(subscriptions::update).delete(subscriptions::delete))        // ── Workflows ────────────────────────────────────────────────────────────
        .route("/db/workflows",           get(workflows::list).post(workflows::create))
        .route("/db/workflows/{id}",       delete(workflows::delete))
        .route("/db/workflows/{id}/steps", post(workflows::add_step))
        // ── Cron jobs ──────────────────────────────────────────────────────────
        .route("/db/cron",             get(cron::list).post(cron::create))
        .route("/db/cron/{id}",         patch(cron::update).delete(cron::delete))
        .route("/db/cron/{id}/trigger", post(cron::trigger))
        // ── Audit trail (state_mutations read surface) ─────────────────────────
        .route("/db/history/{database}/{table}", get(history::history))
        .route("/db/blame/{database}/{table}",   get(history::blame))
        .route("/db/replay/{database}",          get(history::replay))
        // ── Schema introspection ───────────────────────────────────────────────
        .route("/db/schema", get(schema::introspect))
        // ── Debug / engine introspection ────────────────────────────────────────
        .route("/db/debug", get(debug::handler))
        // ── File presigned URLs ───────────────────────────────────────────────
        .route("/files/upload-url",   post(files::upload_url))
        .route("/files/download-url", post(files::download_url))
        // ── Health ────────────────────────────────────────────────────────────
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .route("/version", get(|| async {
            Json(json!({
                "service": "data-engine",
                "commit": std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
                "build_time": std::env::var("BUILD_TIME").unwrap_or_else(|_| "unknown".to_string())
            }))
        }))
        .layer(axum::extract::DefaultBodyLimit::max(1 * 1024 * 1024)) // 1 MB
        .with_state(state)
        .layer(axum_middleware::from_fn(require_service_token))
}
