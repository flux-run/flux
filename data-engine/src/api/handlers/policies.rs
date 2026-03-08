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

// ─── GET /db/policies ────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let rows = sqlx::query(
        "SELECT id, table_name, role, operation, allowed_columns, row_condition \
         FROM fluxbase_internal.policies \
         WHERE tenant_id = $1 AND project_id = $2 \
         ORDER BY table_name, role, operation",
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .fetch_all(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    let policies: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            let id: Uuid                        = r.get("id");
            let table_name: String              = r.get("table_name");
            let role: String                    = r.get("role");
            let operation: String               = r.get("operation");
            let allowed_columns: serde_json::Value = r.get("allowed_columns");
            let row_condition: Option<String>   = r.get("row_condition");
            json!({
                "id":              id,
                "table_name":      table_name,
                "role":            role,
                "operation":       operation,
                "allowed_columns": allowed_columns,
                "row_condition":   row_condition,
            })
        })
        .collect();

    Ok(Json(json!({ "policies": policies })))
}

// ─── POST /db/policies ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreatePolicyRequest {
    pub table_name: String,
    pub role: String,
    pub operation: String,
    #[serde(default)]
    pub allowed_columns: Vec<String>,
    pub row_condition: Option<String>,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreatePolicyRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let allowed_json = serde_json::to_value(&body.allowed_columns)
        .map_err(|e| EngineError::Internal(anyhow::anyhow!(e)))?;

    let row = sqlx::query(
        "INSERT INTO fluxbase_internal.policies \
             (tenant_id, project_id, table_name, role, operation, allowed_columns, row_condition) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (tenant_id, project_id, table_name, role, operation) \
         DO UPDATE SET \
             allowed_columns = EXCLUDED.allowed_columns, \
             row_condition   = EXCLUDED.row_condition \
         RETURNING id",
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .bind(&body.table_name)
    .bind(&body.role)
    .bind(&body.operation)
    .bind(allowed_json)
    .bind(&body.row_condition)
    .fetch_one(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    let id: Uuid = row.get("id");

    Ok(Json(json!({
        "id":     id,
        "status": "created",
    })))
}

// ─── DELETE /db/policies/:id ─────────────────────────────────────────────────

pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let affected = sqlx::query(
        "DELETE FROM fluxbase_internal.policies \
         WHERE id = $1 AND tenant_id = $2 AND project_id = $3",
    )
    .bind(id)
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .execute(&state.pool)
    .await
    .map_err(EngineError::Db)?
    .rows_affected();

    if affected == 0 {
        return Err(EngineError::DatabaseNotFound(format!("policy {id}")));
    }

    Ok(Json(json!({ "id": id, "status": "deleted" })))
}
