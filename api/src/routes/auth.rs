use axum::{
    extract::{Extension, State},
    http::StatusCode,
    Json,
};
use crate::types::context::RequestContext;
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
) -> Result<Json<serde_json::Value>, (StatusCode, Json<serde_json::Value>)> {
    if context.firebase_uid == "api_key" {
        return Ok(Json(json!({
            "user_id": context.user_id,
            "email": "cli-api-key@fluxbase.local",
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
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "database_error"}))))?
    .ok_or((StatusCode::NOT_FOUND, Json(json!({"error": "user_not_found"}))))?;

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
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "database_error"}))))?;

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

    Ok(Json(json!({
        "user_id": context.user_id,
        "email": user_record.email,
        "tenants": tenants
    })))
}

pub async fn logout() -> Json<serde_json::Value> {
    Json(json!({ "success": true }))
}
