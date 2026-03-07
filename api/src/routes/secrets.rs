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

struct SecretRow {
    id: Uuid,
    key: String,
    scope: String,
    created_at: Option<chrono::NaiveDateTime>,
}

// ── Payloads ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct CreateSecretPayload {
    pub key: String,
    pub value: String,
}

// ── Helpers ────────────────────────────────────────────────────────────────

type ApiResult<T> = Result<T, (StatusCode, Json<serde_json::Value>)>;

fn db_err() -> (StatusCode, Json<serde_json::Value>) {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database_error"})))
}

// ── Handlers ───────────────────────────────────────────────────────────────

pub async fn list_secrets(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let project_id = context
        .project_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_project"}))))?;

    let records = sqlx::query_as_unchecked!(
        SecretRow,
        r#"
        SELECT id, key, scope, created_at
        FROM secrets
        WHERE project_id = $1
        ORDER BY created_at DESC
        "#,
        project_id
    )
    .fetch_all(&pool)
    .await
    .map_err(|_| db_err())?;

    let secrets: Vec<_> = records
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "id": r.id,
                "key": r.key,
                "scope": r.scope,
                "created_at": r.created_at.map(|d| d.to_string()).unwrap_or_default()
            })
        })
        .collect();

    Ok(Json(serde_json::json!({ "secrets": secrets })))
}

pub async fn create_secret(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<CreateSecretPayload>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let project_id = context
        .project_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_project"}))))?;

    let tenant_id = context
        .tenant_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_tenant"}))))?;

    let secret_id = Uuid::new_v4();

    sqlx::query!(
        r#"
        INSERT INTO secrets (id, tenant_id, project_id, key, encrypted_value, scope, value)
        VALUES ($1, $2, $3, $4, $5, 'project', $5)
        ON CONFLICT (project_id, key) DO UPDATE SET encrypted_value = EXCLUDED.encrypted_value, value = EXCLUDED.value
        "#,
        secret_id,
        tenant_id,
        project_id,
        payload.key,
        payload.value
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok((StatusCode::CREATED, Json(serde_json::json!({ "secret_id": secret_id }))))
}

pub async fn delete_secret(
    Path(key): Path<String>,
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
) -> ApiResult<Json<serde_json::Value>> {
    let project_id = context
        .project_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_project"}))))?;

    sqlx::query!(
        "DELETE FROM secrets WHERE project_id = $1 AND key = $2",
        project_id,
        key
    )
    .execute(&pool)
    .await
    .map_err(|_| db_err())?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}
