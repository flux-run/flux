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
pub struct CreateCronRequest {
    pub name: String,
    /// Standard 5-field cron expression, e.g. "0 * * * *" (every hour).
    pub schedule: String,
    /// "function" | "queue_job"
    pub action_type: String,
    pub action_config: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct PatchCronRequest {
    pub enabled: Option<bool>,
    pub schedule: Option<String>,
}

// ─── GET /db/cron ─────────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT id, name, schedule, action_type, action_config, \
                enabled, last_run_at, next_run_at \
         FROM fluxbase_internal.cron_jobs \
         ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| json!({
            "id":           r.get::<Uuid, _>("id"),
            "name":         r.get::<String, _>("name"),
            "schedule":     r.get::<String, _>("schedule"),
            "action_type":  r.get::<String, _>("action_type"),
            "enabled":      r.get::<bool, _>("enabled"),
            "last_run_at":  r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("last_run_at"),
            "next_run_at":  r.get::<Option<chrono::DateTime<chrono::Utc>>, _>("next_run_at"),
        }))
        .collect();

    Ok(Json(json!({ "cron": data })))
}

// ─── POST /db/cron ────────────────────────────────────────────────────────────

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateCronRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    validate_action_type(&req.action_type)?;
    let next_run = compute_next_from_schedule(&req.schedule)?;

    use sqlx::Row;
    let row = sqlx::query(
        "INSERT INTO fluxbase_internal.cron_jobs \
             (name, schedule, action_type, action_config, next_run_at) \
         VALUES ($1, $2, $3, $4, $5) \
         RETURNING id",
    )
    .bind(&req.name)
    .bind(&req.schedule)
    .bind(&req.action_type)
    .bind(&req.action_config)
    .bind(next_run)
    .fetch_one(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    Ok(Json(json!({ "id": row.get::<Uuid, _>("id") })))
}

// ─── PATCH /db/cron/:id ───────────────────────────────────────────────────────

pub async fn update(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(req): Json<PatchCronRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    if let Some(ref sched) = req.schedule {
        let next = compute_next_from_schedule(sched)?;
        sqlx::query(
            "UPDATE fluxbase_internal.cron_jobs \
             SET schedule = $1, next_run_at = $2, updated_at = now() \
             WHERE id = $3",
        )
        .bind(sched).bind(next).bind(id)
        .execute(&state.pool).await.map_err(EngineError::Db)?;
    }

    if let Some(enabled) = req.enabled {
        sqlx::query(
            "UPDATE fluxbase_internal.cron_jobs \
             SET enabled = $1, updated_at = now() \
             WHERE id = $2",
        )
        .bind(enabled).bind(id)
        .execute(&state.pool).await.map_err(EngineError::Db)?;
    }

    Ok(Json(json!({ "updated": true })))
}

// ─── DELETE /db/cron/:id ──────────────────────────────────────────────────────

pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let r = sqlx::query(
        "DELETE FROM fluxbase_internal.cron_jobs \
         WHERE id = $1",
    )
    .bind(id)
    .execute(&state.pool).await.map_err(EngineError::Db)?;

    if r.rows_affected() == 0 {
        return Err(EngineError::DatabaseNotFound(format!("cron job {}", id)));
    }
    Ok(Json(json!({ "deleted": true })))
}

// ─── POST /db/cron/:id/trigger ───────────────────────────────────────────────────

/// Mark a cron job for immediate execution by setting `next_run_at = NOW()`.
/// The cron worker will pick it up on its next poll cycle.
pub async fn trigger(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let r = sqlx::query(
        "UPDATE fluxbase_internal.cron_jobs \
         SET next_run_at = NOW() \
         WHERE id = $1",
    )
    .bind(id)
    .execute(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    if r.rows_affected() == 0 {
        return Err(EngineError::DatabaseNotFound(format!("cron job {}", id)));
    }
    Ok(Json(json!({ "triggered": true })))
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn compute_next_from_schedule(schedule: &str) -> Result<Option<chrono::DateTime<chrono::Utc>>, EngineError> {
    use std::str::FromStr;
    let six_field = format!("0 {}", schedule);
    cron::Schedule::from_str(&six_field)
        .map_err(|e| EngineError::MissingField(format!("invalid cron expression: {}", e)))
        .map(|s| s.upcoming(chrono::Utc).next())
}

fn validate_action_type(t: &str) -> Result<(), EngineError> {
    match t {
        "function" | "queue_job" => Ok(()),
        other => Err(EngineError::UnsupportedOperation(format!(
            "invalid action_type '{}': expected function | queue_job", other
        ))),
    }
}
