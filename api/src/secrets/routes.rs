use axum::{
    extract::{Extension, Path, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::Value;
use sqlx::PgPool;

use crate::types::context::RequestContext;
use crate::types::response::{ApiResponse, ApiError};
use crate::validation::{
    validate_secret_key, validate_secret_value, PaginationQuery,
};

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

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn map_err(err: ServiceError) -> ApiError {
    match err {
        ServiceError::Database(_) => ApiError::internal("database_error"),
        ServiceError::Encryption(_) => ApiError::internal("encryption_failed"),
        ServiceError::NotFound(msg) => ApiError::not_found(&msg),
        ServiceError::Conflict(msg) => ApiError::new(StatusCode::CONFLICT, "conflict", &msg),
    }
}

// ── Control Plane APIs ──────────────────────────────────────────────────

pub async fn create_secret(
    State(pool): State<PgPool>,
    Extension(_ctx): Extension<RequestContext>,
    Json(payload): Json<CreateSecretRequest>,
) -> ApiResult<Value> {
    validate_secret_key(&payload.key).map_err(|e| ApiError::bad_request(e))?;
    validate_secret_value(&payload.value).map_err(|e| ApiError::bad_request(e))?;

    let (secret_id, version) = svc_create(&pool, payload).await.map_err(map_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "secret_id": secret_id, "version": version })))
}

pub async fn update_secret(
    State(pool): State<PgPool>,
    Extension(_ctx): Extension<RequestContext>,
    Path(key): Path<String>,
    Json(payload): Json<UpdateSecretRequest>,
) -> ApiResult<Value> {
    validate_secret_value(&payload.value).map_err(|e| ApiError::bad_request(e))?;

    let version = svc_update(&pool, &key, payload).await.map_err(map_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "version": version })))
}

pub async fn delete_secret(
    State(pool): State<PgPool>,
    Extension(_ctx): Extension<RequestContext>,
    Path(key): Path<String>,
) -> ApiResult<Value> {
    svc_delete(&pool, &key).await.map_err(map_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "deleted": true })))
}

pub async fn list_secrets(
    State(pool): State<PgPool>,
    Extension(_ctx): Extension<RequestContext>,
    Query(page): Query<PaginationQuery>,
) -> ApiResult<Value> {
    let (limit, offset) = page.clamped();
    let secrets = svc_list(&pool, limit, offset).await.map_err(map_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "secrets": secrets })))
}

// ── Internal Runtime API ────────────────────────────────────────────────

pub async fn get_internal_runtime_secrets(
    headers: HeaderMap,
    State(pool): State<PgPool>,
) -> ApiResult<Value> {
    // Basic service token verification
    let token = headers.get("X-Service-Token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");

    let expected_token = crate::middleware::require_secret(
        "INTERNAL_SERVICE_TOKEN",
        "dev-service-token",
        "Internal service token (INTERNAL_SERVICE_TOKEN)",
    );

    use subtle::ConstantTimeEq;
    if !<bool as From<_>>::from(token.as_bytes().ct_eq(expected_token.as_bytes())) {
        return Err(ApiError::unauthorized("invalid_service_token"));
    }

    let map = svc_get_runtime(&pool)
        .await
        .map_err(map_err)?;

    Ok(ApiResponse::new(serde_json::json!(map)))
}
