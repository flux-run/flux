use axum::{middleware as axum_middleware, routing::{delete, get, patch, post}, Json, Router};
use serde_json::json;
use std::sync::Arc;
use api_contract::routes as R;

use crate::{
    api::{
        handlers::{cron, databases, debug, explain, history, mutations, query, relationships, schema, sql, tables},
        middleware::service_auth::require_service_token,
    },
    state::AppState,
};

pub fn build(state: Arc<AppState>) -> Router {
    Router::new()
        // ── Data API ──────────────────────────────────────────────────────────
        .route(R::db::QUERY.path,            post(query::handler))
        .route(R::db::SQL.path,              post(sql::handler))
        // ── Database management ───────────────────────────────────────────────
        .route(R::db::DATABASES_LIST.path,   post(databases::create).get(databases::list))
        .route(R::db::DATABASES_DELETE.path, delete(databases::drop_db))
        // ── Table management ──────────────────────────────────────────────────
        .route(R::db::TABLES_CREATE.path,    post(tables::create))
        .route(R::db::TABLES_LIST.path,      get(tables::list))
        .route(R::db::TABLES_DELETE.path,    delete(tables::drop_table))
        // ── Relationships ─────────────────────────────────────────────────────
        .route(R::db::RELATIONSHIPS_LIST.path,   get(relationships::list).post(relationships::create))
        .route(R::db::RELATIONSHIPS_DELETE.path, delete(relationships::delete))
        // ── Cron jobs ──────────────────────────────────────────────────────────
        .route(R::db::CRON_LIST.path,    get(cron::list).post(cron::create))
        .route(R::db::CRON_UPDATE.path,  patch(cron::update).delete(cron::delete))
        .route(R::db::CRON_TRIGGER.path, post(cron::trigger))
        // ── Audit trail (state_mutations read surface) ─────────────────────────
        .route(R::db::HISTORY.path,   get(history::history))
        .route(R::db::BLAME.path,     get(history::blame))
        .route(R::db::REPLAY.path,    get(history::replay))
        .route(R::db::MUTATIONS.path, get(mutations::handler))
        // ── Schema introspection ───────────────────────────────────────────────
        .route(R::db::SCHEMA.path,  get(schema::introspect))
        // ── Debug / engine introspection ────────────────────────────────────────
        .route(R::db::DEBUG.path,   get(debug::handler))
        .route(R::db::EXPLAIN.path, post(explain::handler))
        // ── Health ────────────────────────────────────────────────────────────
        .route(R::health::HEALTH.path,  get(|| async { Json(json!({ "status": "ok" })) }))
        .route(R::health::VERSION.path, get(|| async {
            Json(json!({
                "service": "data-engine",
                "commit": std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
                "build_time": std::env::var("BUILD_TIME").unwrap_or_else(|_| "unknown".to_string())
            }))
        }))
        .layer(axum::extract::DefaultBodyLimit::max(1 * 1024 * 1024))
        .with_state(state)
        .layer(axum_middleware::from_fn(require_service_token))
}
