use axum::{
    extract::{State, Json},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;
use crate::engine::executor::execute_function;
use crate::secrets::secrets_client::SecretsClient;

#[derive(Deserialize)]
pub struct ExecuteRequest {
    pub function_id: String,
    pub tenant_id: Uuid,
    pub project_id: Option<Uuid>,
    pub payload: Value,
}

#[derive(Serialize)]
pub struct ExecuteResponse {
    pub result: Value,
    pub duration_ms: u64,
}

pub struct AppState {
    pub secrets_client: SecretsClient,
}

#[axum::debug_handler]
pub async fn execute_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExecuteRequest>,
) -> impl IntoResponse {
    let start_time = std::time::Instant::now();

    let secrets = match state.secrets_client.fetch_secrets(req.tenant_id, req.project_id).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "SecretFetchError", "message": e })),
            ).into_response();
        }
    };

    // MVP Mock Code load (future: fetch from S3)
    let code = r#"
        export default async function(ctx) {
            return {
                status: "success",
                echo: ctx.payload,
                secret_count: Object.keys(ctx.env).length
            };
        }
    "#;

    let result = match execute_function(code.to_string(), secrets, req.payload).await {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "FunctionExecutionError", "message": e })),
            ).into_response();
        }
    };

    let duration_ms = start_time.elapsed().as_millis() as u64;

    (
        StatusCode::OK,
        Json(ExecuteResponse { result, duration_ms }),
    ).into_response()
}

pub async fn health_check() -> &'static str {
    "OK"
}
