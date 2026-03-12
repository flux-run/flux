use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;

use crate::{
    engine::{auth_context::AuthContext, error::EngineError},
    router::DbRouter,
    state::AppState,
};

// ─── POST /db/databases ───────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateDatabaseRequest {
    pub name: String,
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(body): Json<CreateDatabaseRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let schema = DbRouter::schema_name(&body.name)?;
    DbRouter::create_schema(&state.pool, &schema).await?;

    Ok(Json(json!({
        "database": body.name,
        "schema":   schema,
        "status":   "created",
    })))
}

// ─── GET /db/databases ────────────────────────────────────────────────────────

pub async fn list(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let schemas = DbRouter::list_schemas(&state.pool).await?;
    Ok(Json(json!({ "databases": schemas })))
}

// ─── DELETE /db/databases/:name ───────────────────────────────────────────────

pub async fn drop_db(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let schema = DbRouter::schema_name(&name)?;
    DbRouter::assert_exists(&state.pool, &schema).await?;
    DbRouter::drop_schema(&state.pool, &schema).await?;

    Ok(Json(json!({ "database": name, "status": "dropped" })))
}
