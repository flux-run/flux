//! `/agents/*` — deploy, list, get, delete, and run agents.
//!
//! Deploy:   POST /agents        (body: raw YAML)
//! List:     GET  /agents
//! Get:      GET  /agents/{name}
//! Delete:   DELETE /agents/{name}
//! Run:      POST /agents/{name}/run     (body: JSON input)
//! Simulate: POST /agents/{name}/simulate

use std::sync::Arc;

use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
};
use serde::Deserialize;
use sqlx::PgPool;

use job_contract::dispatch::AgentDispatch;

use crate::error::{ApiError, ApiResponse, ApiResult};
use crate::types::context::RequestContext;

fn db_err(e: impl std::fmt::Display) -> ApiError {
    ApiError::internal(e.to_string())
}

// ── POST /agents — deploy from YAML body ──────────────────────────────────────

pub async fn agent_deploy(
    State(pool): State<PgPool>,
    Extension(ctx): Extension<RequestContext>,
    body: String,
) -> ApiResult<serde_json::Value> {
    let _ = ctx; // project-scoped auth already validated by middleware
    let agent = agent::registry::deploy_from_yaml(&pool, &body)
        .await
        .map_err(ApiError::bad_request)?;

    Ok(ApiResponse::created(serde_json::json!({
        "name":        agent.name,
        "model":       agent.model,
        "tools":       agent.tools,
        "llm_url":     agent.llm_url,
        "llm_secret":  agent.llm_secret,
        "max_turns":   agent.max_turns,
        "temperature": agent.temperature,
    })))
}

// ── GET /agents — list deployed agents ───────────────────────────────────────

pub async fn agents_list(
    State(pool): State<PgPool>,
    Extension(_ctx): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let agents = agent::registry::list_agents(&pool)
        .await
        .map_err(db_err)?;

    let count = agents.len();
    Ok(ApiResponse::new(serde_json::json!({
        "data":  agents,
        "count": count,
    })))
}

// ── GET /agents/{name} ────────────────────────────────────────────────────────

pub async fn agent_get(
    State(pool): State<PgPool>,
    Extension(_ctx): Extension<RequestContext>,
    Path(name): Path<String>,
) -> ApiResult<serde_json::Value> {
    let agent = agent::registry::get_agent(&pool, &name)
        .await
        .map_err(db_err)?
        .ok_or_else(|| ApiError::not_found(format!("agent `{}` not found", name)))?;

    Ok(ApiResponse::new(serde_json::to_value(&agent).unwrap_or_default()))
}

// ── DELETE /agents/{name} ─────────────────────────────────────────────────────

pub async fn agent_delete(
    State(pool): State<PgPool>,
    Extension(_ctx): Extension<RequestContext>,
    Path(name): Path<String>,
) -> Result<StatusCode, ApiError> {
    let deleted = agent::registry::delete_agent(&pool, &name)
        .await
        .map_err(db_err)?;

    if deleted {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found(format!("agent `{}` not found", name)))
    }
}

// ── POST /agents/{name}/run ───────────────────────────────────────────────────

#[derive(Deserialize, Default)]
pub struct RunInput {
    #[serde(default)]
    pub input: serde_json::Value,
    /// Optional idempotency / correlation key.  Auto-generated if not provided.
    pub request_id: Option<String>,
}

pub async fn agent_run(
    State(pool): State<PgPool>,
    Extension(ctx): Extension<RequestContext>,
    Extension(agent_dispatch): Extension<Arc<dyn AgentDispatch>>,
    Path(name): Path<String>,
    axum::Json(body): axum::Json<RunInput>,
) -> ApiResult<serde_json::Value> {
    let request_id = body.request_id
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    // Fetch decrypted secrets for this project
    let secrets = crate::secrets::service::get_runtime_secrets(
        &pool,
        ctx.tenant_id,
        Some(ctx.project_id),
    )
    .await
    .map_err(|e| ApiError::internal(format!("{:?}", e)))?;

    let output = agent_dispatch
        .run(&name, body.input, &request_id, ctx.project_id, secrets)
        .await
        .map_err(ApiError::internal)?;

    Ok(ApiResponse::new(serde_json::json!({
        "output":     output,
        "request_id": request_id,
    })))
}

// ── POST /agents/{name}/simulate ─────────────────────────────────────────────

/// Simulate runs the same code path as /run but returns the full message
/// transcript alongside the output — useful for debugging without side effects.
///
/// In v1 this is identical to /run.  A future version will support
/// dry-run tool calls (mock responses).
pub async fn agent_simulate(
    State(pool): State<PgPool>,
    Extension(ctx): Extension<RequestContext>,
    Extension(agent_dispatch): Extension<Arc<dyn AgentDispatch>>,
    Path(name): Path<String>,
    axum::Json(body): axum::Json<RunInput>,
) -> ApiResult<serde_json::Value> {
    let request_id = body.request_id
        .unwrap_or_else(|| format!("sim-{}", uuid::Uuid::new_v4()));

    let secrets = crate::secrets::service::get_runtime_secrets(
        &pool,
        ctx.tenant_id,
        Some(ctx.project_id),
    )
    .await
    .map_err(|e| ApiError::internal(format!("{:?}", e)))?;

    let output = agent_dispatch
        .run(&name, body.input, &request_id, ctx.project_id, secrets)
        .await
        .map_err(ApiError::internal)?;

    Ok(ApiResponse::new(serde_json::json!({
        "output":     output,
        "request_id": request_id,
        "simulated":  true,
    })))
}
