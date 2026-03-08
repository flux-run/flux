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
    pub control_plane_url: String,
    pub service_token: String,
    pub bundle_cache: crate::cache::bundle_cache::BundleCache,
}

#[axum::debug_handler]
pub async fn execute_handler(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ExecuteRequest>,
) -> impl IntoResponse {
    let start_time = std::time::Instant::now();

    // Fetch secrets from the control plane
    let secrets = match state.secrets_client.fetch_secrets(req.tenant_id, req.project_id).await {
        Ok(s) => s,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "SecretFetchError", "message": e })),
            ).into_response();
        }
    };

    // Fetch real bundle code from the control plane
    let bundle_url = format!(
        "{}/internal/bundle?function_id={}",
        state.control_plane_url, req.function_id
    );

    let http_client = reqwest::Client::new();
    let bundle_resp = http_client
        .get(&bundle_url)
        .header("X-Service-Token", &state.service_token)
        .send()
        .await;

    let code = match bundle_resp {
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "BundleFetchError",
                    "message": format!("Failed to reach control plane: {}", e)
                })),
            ).into_response();
        }
        Ok(resp) => {
            if resp.status() == reqwest::StatusCode::NOT_FOUND {
                return (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({
                        "error": "no_bundle_found",
                        "message": "No active deployment found for this function. Deploy it first."
                    })),
                ).into_response();
            }
            if !resp.status().is_success() {
                let status = resp.status().as_u16();
                let body = resp.text().await.unwrap_or_default();
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "BundleFetchError",
                        "message": format!("Control plane returned HTTP {}: {}", status, body)
                    })),
                ).into_response();
            }
            let json: serde_json::Value = match resp.json().await {
                Ok(j) => j,
                Err(e) => {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "error": "BundleParseError",
                            "message": format!("Failed to parse bundle response: {}", e)
                        })),
                    ).into_response();
                }
            };
            let (deployment_id, url_opt, code_opt) = {
                let d_id = json.get("data").and_then(|d| d.get("deployment_id")).and_then(|id| id.as_str());
                let u_id = json.get("data").and_then(|d| d.get("url")).and_then(|u| u.as_str());
                let c_id = json.get("data").and_then(|d| d.get("code")).and_then(|c| c.as_str());
                (d_id.map(|s| s.to_string()), u_id.map(|s| s.to_string()), c_id.map(|s| s.to_string()))
            };

            let final_code = if let Some(d_id) = deployment_id.clone() {
                if let Some(cached_code) = state.bundle_cache.get(&d_id) {
                    println!("[CACHE HIT] Bundle {}", d_id);
                     Some(cached_code)
                } else { None }
            } else { None };

            if let Some(c) = final_code {
                c
            } else if let Some(url) = url_opt {
                let s3_resp = http_client.get(&url).send().await;
                match s3_resp {
                    Ok(res) if res.status().is_success() => {
                        let text = res.text().await.unwrap_or_default();
                        if let Some(d_id) = deployment_id {
                            println!("[CACHE MISS] Downloaded bundle {}", d_id);
                            state.bundle_cache.insert(d_id, text.clone());
                        }
                        text
                    }
                    Ok(res) => {
                        let status = res.status().as_u16();
                        let body = res.text().await.unwrap_or_default();
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({
                                "error": "S3FetchError",
                                "message": format!("S3 returned HTTP {}: {}", status, body)
                            })),
                        ).into_response();
                    }
                    Err(e) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(serde_json::json!({
                                "error": "S3FetchError",
                                "message": format!("Failed to download bundle from R2/S3 presigned URL: {}", e)
                            })),
                        ).into_response();
                    }
                }
            } else if let Some(code_str) = code_opt {
                // Fallback to inline database storage
                if let Some(d_id) = deployment_id {
                    state.bundle_cache.insert(d_id, code_str.clone());
                }
                code_str
            } else {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({
                        "error": "no_bundle_found",
                        "message": "Bundle response did not contain url or code field"
                    })),
                ).into_response();
            }
        }
    };

    // Execute the function with the new framework-aware executor
    let execution = match execute_function(code, secrets, req.payload).await {
        Ok(r) => r,
        Err(e) => {
            // Parse structured error from the framework if available
            let (err_code, message) = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&e) {
                let code = parsed.get("code").and_then(|c| c.as_str()).unwrap_or("FunctionExecutionError").to_string();
                let msg = parsed.get("message").and_then(|m| m.as_str()).unwrap_or(&e).to_string();
                (code, msg)
            } else {
                ("FunctionExecutionError".to_string(), e)
            };

            let status = if err_code == "INPUT_VALIDATION_ERROR" {
                StatusCode::BAD_REQUEST
            } else if err_code == "OUTPUT_VALIDATION_ERROR" {
                StatusCode::INTERNAL_SERVER_ERROR
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };

            return (status, Json(serde_json::json!({ "error": err_code, "message": message }))).into_response();
        }
    };

    let duration_ms = start_time.elapsed().as_millis() as u64;

    // Fire-and-forget: forward ctx.log() lines to /internal/logs
    if !execution.logs.is_empty() {
        let log_url = format!("{}/internal/logs", state.control_plane_url);
        let service_token = state.service_token.clone();
        let function_id = req.function_id.clone();
        let logs = execution.logs;
        let client = http_client;

        tokio::spawn(async move {
            for log in logs {
                let _ = client
                    .post(&log_url)
                    .header("X-Service-Token", &service_token)
                    .json(&serde_json::json!({
                        "function_id": function_id,
                        "level": log.level,
                        "message": log.message,
                    }))
                    .send()
                    .await;
            }
        });
    }

    (
        StatusCode::OK,
        Json(ExecuteResponse { result: execution.output, duration_ms }),
    ).into_response()
}

pub async fn health_check() -> &'static str {
    "OK"
}
