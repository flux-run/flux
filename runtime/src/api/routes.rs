use axum::{
    extract::{State, Json},
    http::StatusCode,
    response::IntoResponse,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;
use crate::engine::pool::IsolatePool;
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
    pub http_client: reqwest::Client,
    pub control_plane_url: String,
    pub service_token: String,
    pub bundle_cache: crate::cache::bundle_cache::BundleCache,
    pub isolate_pool: IsolatePool,
}

#[axum::debug_handler]
pub async fn execute_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<ExecuteRequest>,
) -> impl IntoResponse {
    let tenant_id_header = headers.get("X-Tenant-Id")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_else(|| "unknown");
    let tenant_slug_header = headers.get("X-Tenant-Slug")
        .and_then(|h| h.to_str().ok())
        .unwrap_or_else(|| "unknown");
    let request_id = headers.get("x-request-id")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let start_time = std::time::Instant::now();

    // ── Function-level bundle cache (skips control plane + S3 entirely) ──
    if let Some(cached_code) = state.bundle_cache.get_by_function(&req.function_id) {
        tracing::debug!(function_id = %req.function_id, "bundle cache hit (function-level)");
        let secrets = match state.secrets_client.fetch_secrets(req.tenant_id, req.project_id).await {
            Ok(s) => s,
            Err(e) => return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": "SecretFetchError", "message": e })),
            ).into_response(),
        };
        let execution = match state.isolate_pool.execute(
            cached_code,
            secrets,
            req.payload,
            tenant_id_header.to_string(),
            tenant_slug_header.to_string(),
        ).await {
            Ok(r) => r,
            Err(e) => {
                let (err_code, message) = if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&e) {
                    let code = parsed.get("code").and_then(|c| c.as_str()).unwrap_or("FunctionExecutionError").to_string();
                    let msg  = parsed.get("message").and_then(|m| m.as_str()).unwrap_or(&e).to_string();
                    (code, msg)
                } else {
                    ("FunctionExecutionError".to_string(), e)
                };
                let status = if err_code == "INPUT_VALIDATION_ERROR" { StatusCode::BAD_REQUEST } else { StatusCode::INTERNAL_SERVER_ERROR };
                return (status, Json(serde_json::json!({ "error": err_code, "message": message }))).into_response();
            }
        };
        let duration_ms = start_time.elapsed().as_millis() as u64;
        if !execution.logs.is_empty() {
            let log_url       = format!("{}/internal/logs", state.control_plane_url);
            let service_token = state.service_token.clone();
            let function_id   = req.function_id.clone();
            let tenant_id     = req.tenant_id;
            let project_id    = req.project_id;
            let logs          = execution.logs;
            let client        = state.http_client.clone();
            let request_id_log = request_id.clone();
            tokio::spawn(async move {
                for log in logs {
                    let _ = client.post(&log_url).header("X-Service-Token", &service_token)
                        .json(&serde_json::json!({
                            "source":      "function",
                            "resource_id": function_id,
                            "tenant_id":   tenant_id,
                            "project_id":  project_id,
                            "level":       log.level,
                            "message":     log.message,
                            "request_id":  request_id_log,
                        }))
                        .send().await;
                }
            });
        }
        return (StatusCode::OK, Json(ExecuteResponse { result: execution.output, duration_ms })).into_response();
    }

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

    let bundle_resp = state.http_client
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
                    tracing::debug!(function_id = %req.function_id, deployment_id = %d_id, "bundle cache hit (deployment-level) — re-warming function cache");
                    // Re-warm the function-level cache so the next call skips the control plane.
                    state.bundle_cache.insert_both(req.function_id.clone(), Some(d_id), cached_code.clone());
                    Some(cached_code)
                } else { None }
            } else { None };

            if let Some(c) = final_code {
                c
            } else if let Some(url) = url_opt {
                let s3_resp = state.http_client.get(&url).send().await;
                match s3_resp {
                    Ok(res) if res.status().is_success() => {
                        let text = res.text().await.unwrap_or_default();
                        tracing::debug!(function_id = %req.function_id, deployment_id = ?deployment_id, "bundle cache miss — caching");
                        state.bundle_cache.insert_both(req.function_id.clone(), deployment_id, text.clone());
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
                tracing::debug!(function_id = %req.function_id, "bundle cache miss (inline) — caching");
                state.bundle_cache.insert_both(req.function_id.clone(), deployment_id, code_str.clone());
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
    let execution = match state.isolate_pool.execute(
        code, 
        secrets, 
        req.payload,
        tenant_id_header.to_string(),
        tenant_slug_header.to_string(),
    ).await {
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
        let log_url        = format!("{}/internal/logs", state.control_plane_url);
        let service_token  = state.service_token.clone();
        let function_id    = req.function_id.clone();
        let tenant_id      = req.tenant_id;
        let project_id     = req.project_id;
        let logs           = execution.logs;
        let client         = state.http_client.clone();
        let request_id_log = request_id.clone();

        tokio::spawn(async move {
            for log in logs {
                let _ = client
                    .post(&log_url)
                    .header("X-Service-Token", &service_token)
                    .json(&serde_json::json!({
                        "source":      "function",
                        "resource_id": function_id,
                        "tenant_id":   tenant_id,
                        "project_id":  project_id,
                        "level":       log.level,
                        "message":     log.message,
                        "request_id":  request_id_log,
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

pub async fn health_check() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

/// POST /internal/cache/invalidate
///
/// Called by the control plane immediately after a new deployment goes live,
/// so the runtime stops serving the old bundle within milliseconds instead of
/// waiting for the 60-second function-cache TTL to expire.
///
/// Body (all fields optional – omit any you don't want to invalidate):
/// ```json
/// { "function_id": "...", "deployment_id": "...",
///   "tenant_id":   "...", "project_id":    "..." }
/// ```
#[derive(Deserialize)]
pub struct InvalidateCacheRequest {
    pub function_id:   Option<String>,
    pub deployment_id: Option<String>,
    pub tenant_id:     Option<uuid::Uuid>,
    pub project_id:    Option<uuid::Uuid>,
}

pub async fn invalidate_cache_handler(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Json(req): Json<InvalidateCacheRequest>,
) -> impl IntoResponse {
    // Require the service token so this endpoint is not publicly callable.
    let provided = headers
        .get("X-Service-Token")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    if provided != state.service_token {
        return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({ "error": "unauthorized" }))).into_response();
    }

    let mut evicted: Vec<&str> = Vec::new();

    if let Some(ref fid) = req.function_id {
        state.bundle_cache.invalidate_function(fid);
        evicted.push("function_bundle");
    }
    if let Some(ref did) = req.deployment_id {
        state.bundle_cache.invalidate_deployment(did);
        evicted.push("deployment_bundle");
    }
    if let Some(tid) = req.tenant_id {
        state.secrets_client.cache().invalidate(tid, req.project_id);
        evicted.push("secrets");
    }

    tracing::info!(
        function_id   = ?req.function_id,
        deployment_id = ?req.deployment_id,
        tenant_id     = ?req.tenant_id,
        "cache invalidated: {:?}", evicted
    );

    (StatusCode::OK, Json(serde_json::json!({ "evicted": evicted }))).into_response()
}
