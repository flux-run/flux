use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use crate::types::context::RequestContext;
use crate::types::response::{ApiResponse, ApiError};
use serde_json::json;
use sqlx::PgPool;

struct UserEmailRow {
    email: String,
}

struct TenantMembershipRow {
    tenant_id: uuid::Uuid,
    name: String,
    role: String,
}

pub async fn get_me(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> Result<ApiResponse<serde_json::Value>, ApiError> {
    if context.firebase_uid == "api_key" {
        let mut tenant_slug = None;
        if let Some(tid) = context.tenant_id {
            #[derive(sqlx::FromRow)]
            struct SlugRow { slug: String }
            
            if let Ok(Some(row)) = sqlx::query_as::<_, SlugRow>("SELECT slug FROM tenants WHERE id = $1")
                .bind(tid)
                .fetch_optional(&pool)
                .await
            {
                tenant_slug = Some(row.slug);
            }
        }

        return Ok(ApiResponse::new(json!({
            "user_id": context.user_id,
            "email": "cli-api-key@fluxbase.local",
            "tenant_id": context.tenant_id,
            "tenant_slug": tenant_slug,
            "project_id": context.project_id,
            "tenants": []
        })));
    }

    let user_record = sqlx::query_as_unchecked!(
        UserEmailRow,
        "SELECT email FROM users WHERE id = $1",
        context.user_id
    )
    .fetch_optional(&pool)
    .await
    .map_err(|_| ApiError::internal("database_error"))?
    .ok_or(ApiError::not_found("user_not_found"))?;

    let tenant_records = sqlx::query_as_unchecked!(
        TenantMembershipRow,
        r#"
        SELECT t.id as tenant_id, t.name, tm.role
        FROM tenants t
        JOIN tenant_members tm ON t.id = tm.tenant_id
        WHERE tm.user_id = $1
        "#,
        context.user_id
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| ApiError::internal("database_error"))?;

    let tenants: Vec<serde_json::Value> = tenant_records
        .into_iter()
        .map(|t| {
            json!({
                "tenant_id": t.tenant_id,
                "name": t.name,
                "role": t.role
            })
        })
        .collect();

    Ok(ApiResponse::new(json!({
        "user_id": context.user_id,
        "email": user_record.email,
        "tenants": tenants
    })))
}

pub async fn logout() -> ApiResponse<serde_json::Value> {
    ApiResponse::new(json!({ "success": true }))
}
