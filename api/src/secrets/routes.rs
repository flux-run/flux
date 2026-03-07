use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

use crate::types::context::RequestContext;

use super::{
    dto::{CreateSecretRequest, UpdateSecretRequest},
    service::{
        create_secret as svc_create,
        delete_secret as svc_delete,
        list_secrets as svc_list,
        update_secret as svc_update,
        get_runtime_secrets as svc_get_runtime,
        ServiceError,
    },
};

type ApiResult<T> = Result<T, (StatusCode, Json<serde_json::Value>)>;

fn map_err(err: ServiceError) -> (StatusCode, Json<serde_json::Value>) {
    match err {
        ServiceError::Database(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "database_error"})),
        ),
        ServiceError::Encryption(_) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "encryption_failed"})),
        ),
        ServiceError::NotFound(msg) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": msg })),
        ),
        ServiceError::Conflict(msg) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": msg })),
        ),
    }
}

// ── Control Plane APIs ──────────────────────────────────────────────────

pub async fn create_secret(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Json(payload): Json<CreateSecretRequest>,
) -> ApiResult<(StatusCode, Json<Value>)> {
    let tenant_id = context
        .tenant_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_tenant"}))))?;

    let (secret_id, version) = svc_create(&pool, tenant_id, payload).await.map_err(map_err)?;

    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({ "secret_id": secret_id, "version": version })),
    ))
}

pub async fn update_secret(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Path(key): Path<String>,
    Json(payload): Json<UpdateSecretRequest>,
) -> ApiResult<Json<Value>> {
    let tenant_id = context
        .tenant_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_tenant"}))))?;

    let version = svc_update(&pool, tenant_id, &key, payload).await.map_err(map_err)?;

    Ok(Json(serde_json::json!({ "version": version })))
}

#[derive(Deserialize)]
pub struct DeleteSecretQuery {
    project_id: Option<Uuid>,
}

pub async fn delete_secret(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Path(key): Path<String>,
    Query(query): Query<DeleteSecretQuery>,
) -> ApiResult<Json<Value>> {
    let tenant_id = context
        .tenant_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_tenant"}))))?;

    svc_delete(&pool, tenant_id, query.project_id, &key).await.map_err(map_err)?;

    Ok(Json(serde_json::json!({ "deleted": true })))
}

#[derive(Deserialize)]
pub struct ListSecretsQuery {
    project_id: Option<Uuid>,
}

pub async fn list_secrets(
    State(pool): State<PgPool>,
    Extension(context): Extension<RequestContext>,
    Query(query): Query<ListSecretsQuery>,
) -> ApiResult<Json<Value>> {
    let tenant_id = context
        .tenant_id
        .ok_or((StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": "missing_tenant"}))))?;

    let secrets = svc_list(&pool, tenant_id, query.project_id).await.map_err(map_err)?;

    Ok(Json(serde_json::json!({ "secrets": secrets })))
}

// ── Internal Runtime API ────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct InternalRuntimeSecretQuery {
    tenant_id: Uuid,
    project_id: Option<Uuid>,
}

pub async fn get_internal_runtime_secrets(
    headers: HeaderMap,
    State(pool): State<PgPool>,
    Query(query): Query<InternalRuntimeSecretQuery>,
) -> ApiResult<Json<Value>> {
    // Basic service token verification
    let token = headers.get("X-Service-Token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let expected_token = std::env::var("INTERNAL_SERVICE_TOKEN").unwrap_or_else(|_| "stub_token".to_string());
    
    if token != expected_token {
        return Err((StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "invalid_service_token"}))));
    }

    let map = svc_get_runtime(&pool, query.tenant_id, query.project_id)
        .await
        .map_err(map_err)?;

    Ok(Json(serde_json::json!(map)))
}
