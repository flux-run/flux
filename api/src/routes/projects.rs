use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use crate::types::response::{ApiResponse, ApiError};
use serde::Deserialize;
use sqlx::PgPool;
use uuid::Uuid;
use crate::types::context::RequestContext;

// ── Row structs ────────────────────────────────────────────────────────────

#[derive(sqlx::FromRow)]
struct ProjectRow {
    id: Uuid,
    name: String,
    slug: String,
}

#[derive(sqlx::FromRow)]
struct ProjectDetailRow {
    id: Uuid,
    name: String,
    slug: String,
    tenant_slug: String,
    created_at: chrono::NaiveDateTime,
}

// ── Payloads ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateProjectPayload {
    pub name: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err() -> ApiError {
    ApiError::internal("database_error")
}

fn missing_tenant() -> ApiError {
    ApiError::bad_request("missing_tenant")
}

// ── Handlers ───────────────────────────────────────────────────────────────

pub async fn create_project(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<CreateProjectPayload>,
) -> ApiResult<serde_json::Value> {
    let tenant_id = context.tenant_id.ok_or_else(missing_tenant)?;
    let project_id = Uuid::new_v4();
    let slug = crate::services::slug_service::generate_slug(&payload.name);

    sqlx::query(
        "INSERT INTO projects (id, tenant_id, name, slug) VALUES ($1, $2, $3, $4)"
    )
    .bind(project_id)
    .bind(tenant_id)
    .bind(payload.name)
    .bind(&slug)
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok(ApiResponse::new(serde_json::json!({ 
        "project_id": project_id,
        "slug": slug
    })))
}

pub async fn get_projects(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let tenant_id = context.tenant_id.ok_or_else(missing_tenant)?;

    let records = sqlx::query_as::<_, ProjectRow>(
        "SELECT id, name, slug FROM projects WHERE tenant_id = $1 ORDER BY created_at DESC",
    )
    .bind(tenant_id)
    .fetch_all(&pool)
    .await
    .map_err(|_| db_err())?;

    let projects: Vec<_> = records
        .into_iter()
        .map(|r| serde_json::json!({ "id": r.id, "name": r.name, "slug": r.slug }))
        .collect();

    Ok(ApiResponse::new(serde_json::json!({ "projects": projects })))
}

pub async fn get_project(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let tenant_id = context.tenant_id.ok_or_else(missing_tenant)?;

    let record = sqlx::query_as::<_, ProjectDetailRow>(
        "SELECT p.id, p.name, p.slug, t.slug as tenant_slug, p.created_at \
         FROM projects p \
         JOIN tenants t ON p.tenant_id = t.id \
         WHERE p.id = $1 AND p.tenant_id = $2",
    )
    .bind(id)
    .bind(tenant_id)
    .fetch_optional(&pool)
    .await
    .map_err(|_| db_err())?
    .ok_or(ApiError::not_found("project_not_found"))?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": record.id,
        "name": record.name,
        "slug": record.slug,
        "tenant_slug": record.tenant_slug,
        "created_at": record.created_at.to_string()
    })))
}

pub async fn delete_project(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let tenant_id = context.tenant_id.ok_or_else(missing_tenant)?;

    if context.role.as_deref() != Some("owner") && context.role.as_deref() != Some("admin") {
        return Err(ApiError::forbidden("forbidden"));
    }

    sqlx::query!(
        "DELETE FROM projects WHERE id = $1 AND tenant_id = $2",
        id,
        tenant_id
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}
