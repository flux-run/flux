use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    engine::{auth_context::AuthContext, error::EngineError},
    state::AppState,
};

#[derive(Debug, Serialize)]
pub struct Relationship {
    pub id: Uuid,
    pub schema_name: String,
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
    /// "one_to_one" | "one_to_many" | "many_to_many"
    pub relationship: String,
    /// Alias used in query results (e.g. "author", "comments").
    pub alias: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateRelationshipRequest {
    pub schema_name: String,
    pub from_table: String,
    pub from_column: String,
    pub to_table: String,
    pub to_column: String,
    /// "one_to_one" | "one_to_many" | "many_to_many"
    pub relationship: String,
    pub alias: String,
}

// ─── GET /db/relationships ────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    use sqlx::Row;
    let rows = sqlx::query(
        "SELECT id, schema_name, from_table, from_column, to_table, to_column, \
                relationship, alias \
         FROM fluxbase_internal.relationships \
         WHERE tenant_id = $1 AND project_id = $2 \
         ORDER BY from_table, alias",
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .fetch_all(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    let data: Vec<serde_json::Value> = rows
        .iter()
        .map(|r| {
            json!({
                "id":           r.get::<Uuid, _>("id"),
                "schema_name":  r.get::<String, _>("schema_name"),
                "from_table":   r.get::<String, _>("from_table"),
                "from_column":  r.get::<String, _>("from_column"),
                "to_table":     r.get::<String, _>("to_table"),
                "to_column":    r.get::<String, _>("to_column"),
                "relationship": r.get::<String, _>("relationship"),
                "alias":        r.get::<String, _>("alias"),
            })
        })
        .collect();

    Ok(Json(json!({ "data": data })))
}

// ─── POST /db/relationships ───────────────────────────────────────────────────

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<CreateRelationshipRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    use sqlx::Row;
    let row = sqlx::query(
        "INSERT INTO fluxbase_internal.relationships \
             (tenant_id, project_id, schema_name, from_table, from_column, \
              to_table, to_column, relationship, alias) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9) \
         RETURNING id",
    )
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .bind(&req.schema_name)
    .bind(&req.from_table)
    .bind(&req.from_column)
    .bind(&req.to_table)
    .bind(&req.to_column)
    .bind(&req.relationship)
    .bind(&req.alias)
    .fetch_one(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    let id: Uuid = row.get("id");
    state.invalidate_tenant_schema(auth.tenant_id, auth.project_id).await;
    Ok(Json(json!({ "id": id })))
}

// ─── DELETE /db/relationships/:id ─────────────────────────────────────────────

pub async fn delete(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let result = sqlx::query(
        "DELETE FROM fluxbase_internal.relationships \
         WHERE id = $1 AND tenant_id = $2 AND project_id = $3",
    )
    .bind(id)
    .bind(auth.tenant_id)
    .bind(auth.project_id)
    .execute(&state.pool)
    .await
    .map_err(EngineError::Db)?;

    if result.rows_affected() == 0 {
        return Err(EngineError::DatabaseNotFound(format!("relationship {}", id)));
    }

    state.invalidate_tenant_schema(auth.tenant_id, auth.project_id).await;
    Ok(Json(json!({ "deleted": true })))
}
