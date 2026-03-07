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

struct FunctionRow {
    id: Uuid,
    name: String,
    runtime: String,
    created_at: chrono::NaiveDateTime,
}

// ── Payloads ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateFunctionPayload {
    pub name: String,
    pub runtime: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

type ApiResult<T> = Result<T, (StatusCode, Json<serde_json::Value>)>;

fn db_err() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database_error"})))
}

// ── Handlers ───────────────────────────────────────────────────────────────

pub async fn list_functions(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let project_id = context
        .project_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_project"}))))?;

    let records = sqlx::query_as_unchecked!(
        FunctionRow,
        "SELECT id, name, runtime, created_at FROM functions WHERE project_id = $1 ORDER BY created_at DESC",
        project_id
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| db_err())?;

    let functions: Vec<_> = records
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "name": r.name,
                "runtime": r.runtime,
                "created_at": r.created_at.to_string()
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "functions": functions })))
}

pub async fn create_function(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<CreateFunctionPayload>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let project_id = context
        .project_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_project"}))))?;

    let tenant_id = context
        .tenant_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_tenant"}))))?;

    let function_id = Uuid::new_v4();

    sqlx::query!(
        "INSERT INTO functions (id, tenant_id, project_id, name, runtime) VALUES ($1, $2, $3, $4, $5)",
        function_id,
        tenant_id,
        project_id,
        payload.name,
        payload.runtime
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "function_id": function_id }))))
}

pub async fn get_function(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let project_id = context
        .project_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_project"}))))?;

    let record = sqlx::query_as_unchecked!(
        FunctionRow,
        "SELECT id, name, runtime, created_at FROM functions WHERE id = $1 AND project_id = $2",
        id,
        project_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(|_| db_err())?
    .ok_or((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "function_not_found"}))))?;

    Ok(Json(serde_json::json!({
        "id": record.id,
        "name": record.name,
        "runtime": record.runtime,
        "created_at": record.created_at.to_string()
    })))
}

pub async fn delete_function(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let project_id = context
        .project_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_project"}))))?;

    sqlx::query!(
        "DELETE FROM functions WHERE id = $1 AND project_id = $2",
        id,
        project_id
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}
