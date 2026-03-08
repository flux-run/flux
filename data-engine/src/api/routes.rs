use axum::{routing::{delete, get, post}, Json, Router};
use serde_json::json;
use std::sync::Arc;

use crate::{
    api::handlers::{databases, policies, query, tables},
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
        // ── Health ────────────────────────────────────────────────────────────
        .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
        .with_state(state)
}
