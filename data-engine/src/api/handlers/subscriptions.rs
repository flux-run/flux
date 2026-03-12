use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    engine::{auth_context::AuthContext, error::EngineError},
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct CreateSubscriptionRequest {
    /// Pattern: "users.inserted" | "users.*" | "*"
    pub event_pattern: String,
    /// "webhook" | "function" | "queue_job"
    pub target_type: String,
    /// Target-specific config JSON.
    pub target_config: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct PatchSubscriptionRequest {
    pub enabled: bool,
}

// ─── GET /db/subscriptions ────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT id, event_pattern, target_type, target_config, enabled, created_at \
         FROM fluxbase_internal.event_subscriptions \
         ORDER BY event_pattern, created_at",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id":            r.get::<Uuid, _>("id"),
                "event_pattern": r.get::<String, _>("event_pattern"),
                "target_type":   r.get::<String, _>("target_type"),
                "target_config": r.get::<serde_json::Value, _>("target_config"),
                "enabled":       r.get::<bool, _>("enabled"),
            })
        })
        .collect();

    Ok(Json(json!({ "data": data })))
}

// ─── POST /db/subscriptions ───────────────────────────────────────────────────

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateSubscriptionRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    validate_target_type(&req.target_type)?;

    use sqlx::Row;
    let row = sqlx::query(
        "INSERT INTO fluxbase_internal.event_subscriptions \
             (event_pattern, target_type, target_config) \
         VALUES ($1, $2, $3) \
         RETURNING id",
    )
    .bind(&req.event_pattern)
    .bind(&req.target_type)
    .bind(&req.target_config)
    .fetch_one(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    let id: Uuid = row.get("id");
    Ok(Json(json!({ "id": id })))
}

// ─── PATCH /db/subscriptions/:id ─────────────────────────────────────────────

pub async fn update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchSubscriptionRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let result = sqlx::query(
        "UPDATE fluxbase_internal.event_subscriptions \
         SET enabled = $1, updated_at = now() \
         WHERE id = $2",
    )
    .bind(req.enabled)
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    if result.rows_affected() == 0 {
        return Err(EngineError::DatabaseNotFound(format!("subscription {}", id)));
    }

    Ok(Json(json!({ "updated": true })))
}

// ─── DELETE /db/subscriptions/:id ────────────────────────────────────────────

pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let result = sqlx::query(
        "DELETE FROM fluxbase_internal.event_subscriptions \
         WHERE id = $1",
    )
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    if result.rows_affected() == 0 {
        return Err(EngineError::DatabaseNotFound(format!("subscription {}", id)));
    }

    Ok(Json(json!({ "deleted": true })))
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn validate_target_type(t: &str) -> Result<(), EngineError> {
    match t {
        "webhook" | "function" | "queue_job" => Ok(()),
        other => Err(EngineError::UnsupportedOperation(format!(
            "invalid target_type '{}': expected webhook | function | queue_job",
            other
        ))),
    }
}
