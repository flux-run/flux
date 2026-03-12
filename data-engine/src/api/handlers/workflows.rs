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
pub struct CreateWorkflowRequest {
    pub name: String,
    pub description: Option<String>,
    /// Event pattern: "users.inserted" | "orders.*" | "*"
    pub trigger_event: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateStepRequest {
    pub step_order: i32,
    pub name: String,
    /// "function" | "queue_job" | "webhook"
    pub action_type: String,
    pub action_config: serde_json::Value,
    pub condition_expr: Option<String>,
}

// ─── GET /db/workflows ────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT w.id, w.name, w.description, w.trigger_event, w.enabled, \
                (SELECT json_agg(json_build_object(\
                    'id', s.id, 'step_order', s.step_order, 'name', s.name, \
                    'action_type', s.action_type, 'action_config', s.action_config \
                ) ORDER BY s.step_order) \
                 FROM fluxbase_internal.workflow_steps s WHERE s.workflow_id = w.id) AS steps \
         FROM fluxbase_internal.workflows w \
         ORDER BY w.name",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| json!({
            "id":            r.get::<Uuid, _>("id"),
            "name":          r.get::<String, _>("name"),
            "description":   r.get::<Option<String>, _>("description"),
            "trigger_event": r.get::<String, _>("trigger_event"),
            "enabled":       r.get::<bool, _>("enabled"),
            "steps":         r.get::<Option<serde_json::Value>, _>("steps")
                              .unwrap_or(json!([])),
        }))
        .collect();

    Ok(Json(json!({ "data": data })))
}

// ─── POST /db/workflows ───────────────────────────────────────────────────────

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateWorkflowRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    use sqlx::Row;
    let row = sqlx::query(
        "INSERT INTO fluxbase_internal.workflows \
             (name, description, trigger_event) \
         VALUES ($1, $2, $3) \
         RETURNING id",
    )
    .bind(&req.name)
    .bind(&req.description)
    .bind(&req.trigger_event)
    .fetch_one(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    Ok(Json(json!({ "id": row.get::<Uuid, _>("id") })))
}

// ─── DELETE /db/workflows/:id ─────────────────────────────────────────────────

pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let r = sqlx::query(
        "DELETE FROM fluxbase_internal.workflows \
         WHERE id = $1",
    )
    .bind(id)
    .execute(&state.pool).await.map_err(EngineError::Db)?;

    if r.rows_affected() == 0 {
        return Err(EngineError::DatabaseNotFound(format!("workflow {}", id)));
    }
    Ok(Json(json!({ "deleted": true })))
}

// ─── POST /db/workflows/:id/steps ────────────────────────────────────────────

pub async fn add_step(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(workflow_id): Path<Uuid>,
    Json(req): Json<CreateStepRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    // Verify the workflow exists.
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM fluxbase_internal.workflows \
         WHERE id = $1",
    )
    .bind(workflow_id)
    .fetch_one(&state.pool).await.map_err(EngineError::Db)?;

    if count == 0 {
        return Err(EngineError::DatabaseNotFound(format!("workflow {}", workflow_id)));
    }

    validate_action_type(&req.action_type)?;

    use sqlx::Row;
    let row = sqlx::query(
        "INSERT INTO fluxbase_internal.workflow_steps \
             (workflow_id, step_order, name, action_type, action_config, condition_expr) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id",
    )
    .bind(workflow_id)
    .bind(req.step_order)
    .bind(&req.name)
    .bind(&req.action_type)
    .bind(&req.action_config)
    .bind(&req.condition_expr)
    .fetch_one(&state.pool).await.map_err(EngineError::Db)?;

    Ok(Json(json!({ "id": row.get::<Uuid, _>("id") })))
}

fn validate_action_type(t: &str) -> Result<(), EngineError> {
    match t {
        "webhook" | "function" | "queue_job" => Ok(()),
        other => Err(EngineError::UnsupportedOperation(format!(
            "invalid action_type '{}': expected webhook | function | queue_job", other
        ))),
    }
}
