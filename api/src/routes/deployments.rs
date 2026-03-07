use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;
use crate::types::context::RequestContext;

// ── Row structs ────────────────────────────────────────────────────────────

struct DeploymentRow {
    id: Uuid,
    version: i32,
    is_active: bool,
    created_at: chrono::NaiveDateTime,
}

// ── Payloads ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateDeploymentPayload {
    pub storage_key: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

type ApiResult<T> = Result<T, (StatusCode, Json<serde_json::Value>)>;

fn db_err() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database_error"})))
}

// ── Handlers ───────────────────────────────────────────────────────────────

pub async fn list_deployments(
    Path(function_id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(_context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let records = sqlx::query_as_unchecked!(
        DeploymentRow,
        "SELECT id, version, is_active, created_at FROM deployments WHERE function_id = $1 ORDER BY version DESC",
        function_id
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| db_err())?;

    let deployments: Vec<_> = records
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "version": r.version,
                "is_active": r.is_active,
                "created_at": r.created_at.to_string()
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "deployments": deployments })))
}

pub async fn create_deployment(
    Path(function_id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(_context): Extension<RequestContext>,
    Json(payload): Json<CreateDeploymentPayload>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let deployment_id = Uuid::new_v4();

    // Get next version number
    struct VersionRow { max: Option<i32> }
    let row = sqlx::query_as_unchecked!(
        VersionRow,
        "SELECT MAX(version) as max FROM deployments WHERE function_id = $1",
        function_id
    )
    .fetch_one(&pool)
    .await
    .map_err(|_| db_err())?;

    let next_version = row.max.unwrap_or(0) + 1;

    sqlx::query!(
        "INSERT INTO deployments (id, function_id, storage_key, version) VALUES ($1, $2, $3, $4)",
        deployment_id,
        function_id,
        payload.storage_key,
        next_version
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "deployment_id": deployment_id,
            "version": next_version
        })),
    ))
}
