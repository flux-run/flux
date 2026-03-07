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

// ── Row structs (module level to satisfy Rust type inference) ──────────────

struct TenantRow {
    id: Uuid,
    name: String,
    role: String,
}

struct TenantDetailRow {
    id: Uuid,
    name: String,
    created_at: chrono::NaiveDateTime,
}

struct MemberRow {
    id: Uuid,
    email: String,
    role: String,
}

struct UserIdRow {
    id: Uuid,
}

// ── Payloads ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateTenantPayload {
    pub name: String,
}

#[derive(Deserialize)]
pub struct InviteMemberPayload {
    pub email: String,
    pub role: String,
}

// ── Handlers ───────────────────────────────────────────────────────────────

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err() -> ApiError {
    ApiError::internal("database_error")
}

pub async fn create_tenant(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<CreateTenantPayload>,
) -> ApiResult<serde_json::Value> {
    let tenant_id = Uuid::new_v4();

    sqlx::query!(
        "INSERT INTO tenants (id, name, owner_id) VALUES ($1, $2, $3)",
        tenant_id,
        payload.name,
        context.user_id
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    sqlx::query!(
        "INSERT INTO tenant_members (tenant_id, user_id, role) VALUES ($1, $2, 'owner')",
        tenant_id,
        context.user_id
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok(ApiResponse::new(serde_json::json!({ "tenant_id": tenant_id })))
}

pub async fn get_tenants(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let records = sqlx::query_as_unchecked!(
        TenantRow,
        r#"
        SELECT t.id, t.name, tm.role
        FROM tenants t
        JOIN tenant_members tm ON t.id = tm.tenant_id
        WHERE tm.user_id = $1
        ORDER BY t.created_at DESC
        "#,
        context.user_id
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| db_err())?;

    let tenants: Vec<_> = records
        .into_iter()
        .map(|r| serde_json::json!({ "id": r.id, "name": r.name, "role": r.role }))
        .collect();

    Ok(ApiResponse::new(serde_json::json!({ "tenants": tenants })))
}

pub async fn get_tenant(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(_context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    let record = sqlx::query_as_unchecked!(
        TenantDetailRow,
        "SELECT id, name, created_at FROM tenants WHERE id = $1",
        id
    )
    .fetch_optional(&pool)
    .await
    .map_err(|_| db_err())?
    .ok_or(ApiError::not_found("tenant_not_found"))?;

    Ok(ApiResponse::new(serde_json::json!({
        "id": record.id,
        "name": record.name,
        "created_at": record.created_at.to_string()
    })))
}

pub async fn delete_tenant(
    Path(id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    if context.role.as_deref() != Some("owner") {
        return Err(ApiError::forbidden("Only owners can delete tenants"));
    }

    sqlx::query!("DELETE FROM tenants WHERE id = $1", id)
        .execute(&pool)
        .await
        .map_err(|_| db_err())?;

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

// ── Members ────────────────────────────────────────────────────────────────

pub async fn get_members(
    Path(tenant_id): Path<Uuid>,
    State(pool): State<PgPool>,
) -> ApiResult<serde_json::Value> {
    let records = sqlx::query_as_unchecked!(
        MemberRow,
        r#"
        SELECT u.id, u.email, tm.role
        FROM tenant_members tm
        JOIN users u ON tm.user_id = u.id
        WHERE tm.tenant_id = $1
        "#,
        tenant_id
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| db_err())?;

    let members: Vec<_> = records
        .into_iter()
        .map(|r| serde_json::json!({ "user_id": r.id, "email": r.email, "role": r.role }))
        .collect();

    Ok(ApiResponse::new(serde_json::json!({ "members": members })))
}

pub async fn invite_member(
    Path(tenant_id): Path<Uuid>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<InviteMemberPayload>,
) -> ApiResult<serde_json::Value> {
    if context.role.as_deref() != Some("owner") && context.role.as_deref() != Some("admin") {
        return Err(ApiError::forbidden("Only admins or owners can invite"));
    }

    let user = sqlx::query_as_unchecked!(
        UserIdRow,
        "SELECT id FROM users WHERE email = $1",
        payload.email
    )
    .fetch_optional(&pool)
    .await
    .map_err(|_| db_err())?
    .ok_or(ApiError::not_found("user_not_found"))?;

    sqlx::query!(
        "INSERT INTO tenant_members (tenant_id, user_id, role) VALUES ($1, $2, $3)
         ON CONFLICT (tenant_id, user_id) DO UPDATE SET role = EXCLUDED.role",
        tenant_id,
        user.id,
        payload.role
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok(ApiResponse::new(serde_json::json!({ "invited": true })))
}

pub async fn remove_member(
    Path((tenant_id, member_id)): Path<(Uuid, Uuid)>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<serde_json::Value> {
    if context.role.as_deref() != Some("owner") && context.role.as_deref() != Some("admin") {
        return Err(ApiError::forbidden("forbidden"));
    }

    sqlx::query!(
        "DELETE FROM tenant_members WHERE tenant_id = $1 AND user_id = $2",
        tenant_id,
        member_id
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}
