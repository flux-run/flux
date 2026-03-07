use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;
use crate::{
    types::{context::RequestContext, response::{ApiResponse, ApiError}},
    AppState,
};

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err<E: std::fmt::Display>(e: E) -> ApiError {
    ApiError::internal(&format!("database_error: {}", e))
}

use super::{
    model::{CreateApiKeyRequest, CreateApiKeyResponse},
    service,
};

pub async fn create_api_key(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Json(payload): Json<CreateApiKeyRequest>,
) -> ApiResult<CreateApiKeyResponse> {
    let tenant_id = ctx.tenant_id.unwrap_or_default();
    let project_id = ctx.project_id.ok_or(ApiError::bad_request("missing_project_id"))?;
    
    let (_record, plaintext_key) = service::create_api_key(&state.pool, tenant_id, project_id, &payload.name)
        .await
        .map_err(db_err)?;

    let response = CreateApiKeyResponse {
        id: Uuid::new_v4(), // Client doesn't need to know the DB UUID if they only need the token right now, but we can pass it
        key: plaintext_key,
    };

    Ok(ApiResponse::new(response))
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
) -> ApiResult<Vec<super::model::ApiKey>> {
    let tenant_id = ctx.tenant_id.unwrap_or_default();
    let project_id = ctx.project_id.ok_or(ApiError::bad_request("missing_project_id"))?;
    
    let keys = service::list_api_keys(&state.pool, tenant_id, project_id)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(keys))
}

pub async fn revoke_api_key(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> ApiResult<serde_json::Value> {
    let tenant_id = ctx.tenant_id.unwrap_or_default();

    service::revoke_api_key(&state.pool, id, tenant_id)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(serde_json::json!({ "message": "API Key revoked successfully" })))
}
