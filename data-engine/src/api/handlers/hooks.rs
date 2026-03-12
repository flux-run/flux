use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use sqlx::Row;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    engine::{auth_context::AuthContext, error::EngineError},
    state::AppState,
};

// ─── GET /db/hooks ────────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let rows = sqlx::query(
        "SELECT id, table_name, event, function_id, enabled, created_at \
         FROM fluxbase_internal.hooks \
         ORDER BY table_name, event",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    let hooks: Vec<serde_json::Value> = rows.iter().map(|r| {
        json!({
            "id":          r.get::<Uuid, _>("id"),
            "table_name":  r.get::<String, _>("table_name"),
            "event":       r.get::<String, _>("event"),
            "function_id": r.get::<Uuid, _>("function_id"),
            "enabled":     r.get::<bool, _>("enabled"),
        })
    }).collect();

    Ok(Json(json!({ "hooks": hooks })))
}

// ─── POST /db/hooks ───────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateHookRequest {
    pub table_name: String,
    /// "before_insert" | "after_insert" | "before_update" | "after_update"
    /// | "before_delete" | "after_delete"
    pub event: String,
    pub function_id: Uuid,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

static VALID_EVENTS: &[&str] = &[
    "before_insert", "after_insert",
    "before_update", "after_update",
    "before_delete", "after_delete",
];

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateHookRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    if !VALID_EVENTS.contains(&body.event.as_str()) {
        return Err(EngineError::UnsupportedOperation(
            format!("invalid hook event '{}'; expected one of: {}", body.event, VALID_EVENTS.join(", "))
        ));
    }

    let row = sqlx::query(
        "INSERT INTO fluxbase_internal.hooks \
             (table_name, event, function_id, enabled) \
         VALUES ($1, $2, $3, $4) \
         ON CONFLICT (table_name, event) \
         DO UPDATE SET function_id = EXCLUDED.function_id, enabled = EXCLUDED.enabled \
         RETURNING id",
    )
    .bind(&body.table_name)
    .bind(&body.event)
    .bind(body.function_id)
    .bind(body.enabled)
    .fetch_one(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    let id: Uuid = row.get("id");
    state.cache.invalidate_all();
    Ok(Json(json!({ "id": id, "status": "created" })))
}

// ─── PATCH /db/hooks/:id — toggle enabled ────────────────────────────────────

#[derive(Deserialize)]
pub struct UpdateHookRequest {
    pub enabled: bool,
}

pub async fn update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(body): Json<UpdateHookRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let affected = sqlx::query(
        "UPDATE fluxbase_internal.hooks \
         SET enabled = $1 \
         WHERE id = $2",
    )
    .bind(body.enabled)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(EngineError::Db)?
    .rows_affected();

    if affected == 0 {
        return Err(EngineError::DatabaseNotFound(format!("hook {}", id)));
    }
    state.cache.invalidate_all();
    Ok(Json(json!({ "id": id, "enabled": body.enabled })))
}

// ─── DELETE /db/hooks/:id ─────────────────────────────────────────────────────

pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let affected = sqlx::query(
        "DELETE FROM fluxbase_internal.hooks \
         WHERE id = $1",
    )
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(EngineError::Db)?
    .rows_affected();

    if affected == 0 {
        return Err(EngineError::DatabaseNotFound(format!("hook {}", id)));
    }

    state.cache.invalidate_all();
    Ok(Json(json!({ "id": id, "status": "deleted" })))
}
