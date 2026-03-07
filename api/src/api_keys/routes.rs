use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    Json,
};
use uuid::Uuid;
use crate::{
    types::context::RequestContext,
    AppState,
};

type ApiResult<T> = Result<T, (StatusCode, Json<serde_json::Value>)>;

fn db_err<E: std::fmt::Display>(e: E) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "database_error", "message": e.to_string()})),
    )
}

use super::{
    model::{CreateApiKeyRequest, CreateApiKeyResponse},
    service,
};

pub async fn create_api_key(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(project_id): Path<Uuid>,
    Json(payload): Json<CreateApiKeyRequest>,
) -> ApiResult<(StatusCode, Json<CreateApiKeyResponse>)> {
    let tenant_id = ctx.tenant_id.unwrap_or_default();
    
    let (_record, plaintext_key) = service::create_api_key(&state.pool, tenant_id, project_id, &payload.name)
        .await
        .map_err(db_err)?;

    let response = CreateApiKeyResponse {
        id: Uuid::new_v4(), // Client doesn't need to know the DB UUID if they only need the token right now, but we can pass it
        key: plaintext_key,
    };

    Ok((StatusCode::CREATED, Json(response)))
}

pub async fn list_api_keys(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(project_id): Path<Uuid>,
) -> ApiResult<(StatusCode, Json<Vec<super::model::ApiKey>>)> {
    let tenant_id = ctx.tenant_id.unwrap_or_default();
    
    let keys = service::list_api_keys(&state.pool, tenant_id, project_id)
        .await
        .map_err(db_err)?;

    Ok((StatusCode::OK, Json(keys)))
}

pub async fn revoke_api_key(
    State(state): State<AppState>,
    Extension(ctx): Extension<RequestContext>,
    Path(id): Path<Uuid>,
) -> ApiResult<(StatusCode, Json<serde_json::Value>)> {
    let tenant_id = ctx.tenant_id.unwrap_or_default();

    service::revoke_api_key(&state.pool, id, tenant_id)
        .await
        .map_err(db_err)?;

    Ok((StatusCode::OK, Json(serde_json::json!({ "message": "API Key revoked successfully" }))))
}
