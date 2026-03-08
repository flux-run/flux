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
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let schema = DbRouter::schema_name(&auth.tenant_slug, &auth.project_slug, &body.name)?;
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
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let schemas = DbRouter::list_schemas(&state.pool, &auth.tenant_slug, &auth.project_slug).await?;

    // Strip the "t_{tenant}_{project}_" prefix to surface the user-facing db name.
    let prefix = format!("t_{}_{}_", auth.tenant_slug, auth.project_slug);
    let names: Vec<&str> = schemas
        .iter()
        .filter_map(|s| s.strip_prefix(&prefix))
        .collect();

    Ok(Json(json!({ "databases": names })))
}

// ─── DELETE /db/databases/:name ───────────────────────────────────────────────

pub async fn drop_db(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;
    let schema = DbRouter::schema_name(&auth.tenant_slug, &auth.project_slug, &name)?;
    DbRouter::assert_exists(&state.pool, &schema).await?;
    DbRouter::drop_schema(&state.pool, &schema).await?;

    Ok(Json(json!({ "database": name, "status": "dropped" })))
}
