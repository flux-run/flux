use axum::{routing::{delete, get, patch, post}, Json, Router};
use serde_json::json;
use std::sync::Arc;

use crate::{
    api::handlers::{databases, files, hooks, policies, query, relationships, subscriptions, tables},
    state::AppState,
};

pub fn build(state: Arc<AppState>) -> Router {
    Router::new()
        // ── Data API ──────────────────────────────────────────────────────────
        .route("/db/query",                   post(query::handler))
        // ── Database management ───────────────────────────────────────────────
        .route("/db/databases",               post(databases::create).get(databases::list))
        .route("/db/databases/:name",         delete(databases::drop_db))
        // ── Table management ──────────────────────────────────────────────────
        .route("/db/tables",                  post(tables::create))
        .route("/db/tables/:database",        get(tables::list))
        .route("/db/tables/:database/:table", delete(tables::drop_table))
        // ── Policy management ─────────────────────────────────────────────────
        .route("/db/policies",               get(policies::list).post(policies::create))
        .route("/db/policies/:id",           delete(policies::delete))
        // ── Hook management ───────────────────────────────────────────────────
        .route("/db/hooks",     get(hooks::list).post(hooks::create))
        .route("/db/hooks/:id", patch(hooks::update).delete(hooks::delete))
        // ── Relationships ─────────────────────────────────────────────────────
        .route("/db/relationships",     get(relationships::list).post(relationships::create))
        .route("/db/relationships/:id", delete(relationships::delete))
        // ── Event subscriptions ─────────────────────────────────────────────
        .route("/db/subscriptions",     get(subscriptions::list).post(subscriptions::create))
        .route("/db/subscriptions/:id", patch(subscriptions::update).delete(subscriptions::delete))
        // ── File presigned URLs ───────────────────────────────────────────────
        .route("/files/upload-url",   post(files::upload_url))
        .route("/files/download-url", post(files::download_url))
        // ── Health ────────────────────────────────────────────────────────────
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .with_state(state)
}
