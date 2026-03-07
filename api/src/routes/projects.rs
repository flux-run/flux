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

struct ProjectRow {
    id: Uuid,
    name: String,
}

struct ProjectDetailRow {
    id: Uuid,
    name: String,
    created_at: chrono::NaiveDateTime,
}

// ── Payloads ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateProjectPayload {
    pub name: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

type ApiResult<T> = Result<T, (StatusCode, Json<serde_json::Value>)>;

fn db_err() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database_error"})))
}

fn missing_tenant() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_tenant"})))
}

// ── Handlers ───────────────────────────────────────────────────────────────

pub async fn create_project(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<CreateProjectPayload>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let tenant_id = context.tenant_id.ok_or_else(missing_tenant)?;
    let project_id = Uuid::new_v4();

    sqlx::query!(
        "INSERT INTO projects (id, tenant_id, name) VALUES ($1, $2, $3)",
        project_id,
        tenant_id,
        payload.name
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "project_id": project_id }))))
}

pub async fn get_projects(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let tenant_id = context.tenant_id.ok_or_else(missing_tenant)?;

    let records = sqlx::query_as_unchecked!(
        ProjectRow,
        "SELECT id, name FROM projects WHERE tenant_id = $1 ORDER BY created_at DESC",
        tenant_id
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| db_err())?;

    let projects: Vec<_> = records
        .into_iter()
        .map(|r| serde_json::json!({ "id": r.id, "name": r.name }))
        .collect();

    Ok(Json(serde_json::json!({ "projects": projects })))
}

pub async fn get_project(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let tenant_id = context.tenant_id.ok_or_else(missing_tenant)?;

    let record = sqlx::query_as_unchecked!(
        ProjectDetailRow,
        "SELECT id, name, created_at FROM projects WHERE id = $1 AND tenant_id = $2",
        id,
        tenant_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(|_| db_err())?
    .ok_or((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "project_not_found"}))))?;

    Ok(Json(serde_json::json!({
        "id": record.id,
        "name": record.name,
        "created_at": record.created_at.to_string()
    })))
}

pub async fn delete_project(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let tenant_id = context.tenant_id.ok_or_else(missing_tenant)?;

    if context.role.as_deref() != Some("owner") && context.role.as_deref() != Some("admin") {
        return Err((StatusCode::FORBIDDEN, Json(serde_json::json!({"error": "forbidden"}))));
    }

    sqlx::query!(
        "DELETE FROM projects WHERE id = $1 AND tenant_id = $2",
        id,
        tenant_id
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}
