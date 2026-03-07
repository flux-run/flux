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
    status: String,
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
        "SELECT id, version, is_active, status, created_at FROM deployments WHERE function_id = $1 ORDER BY version DESC",
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
                "status": r.status,
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
        "INSERT INTO deployments (id, function_id, storage_key, version, status) VALUES ($1, $2, $3, $4, 'ready')",
        deployment_id,
        function_id,
        payload.storage_key,
        next_version
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    // TODO: Publish to actual event bus
    println!(r#"{{"event": "function.deployed", "function_id": "{}", "deployment_id": "{}"}}"#, function_id, deployment_id);

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({
            "deployment_id": deployment_id,
            "version": next_version
        })),
    ))
}

pub async fn activate_deployment(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(_context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let mut tx = pool.begin().await.map_err(|_| db_err())?;

    // Find the function_id for this deployment to deactivate others
    struct DeploymentFunctionRow { function_id: Uuid }
    let fn_record = sqlx::query_as_unchecked!(
        DeploymentFunctionRow,
        "SELECT function_id FROM deployments WHERE id = $1",
        id
    )
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| db_err())?
    .ok_or((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "deployment_not_found"}))))?;

    // Deactivate all deployments for this function
    sqlx::query!(
        "UPDATE deployments SET is_active = false WHERE function_id = $1",
        fn_record.function_id
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| db_err())?;

    // Activate the requested deployment
    sqlx::query!(
        "UPDATE deployments SET is_active = true WHERE id = $1",
        id
    )
    .execute(&mut *tx)
    .await
    .map_err(|_| db_err())?;

    tx.commit().await.map_err(|_| db_err())?;

    // TODO: Publish to event bus
    println!(r#"{{"event": "function.activated", "function_id": "{}", "deployment_id": "{}"}}"#, fn_record.function_id, id);

    Ok(Json(serde_json::json!({ "activated": true })))
}
