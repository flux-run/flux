use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Result, bail};
use axum::extract::{Path, State, OriginalUri};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use axum::body::to_bytes;

use crate::artifact::RuntimeArtifact;
use crate::deno_runtime::{boot_runtime_artifact, FetchCheckpoint, NetRequest};
use crate::isolate_pool::{ExecutionContext, IsolatePool, ExecutionResult, execute_one_shot_artifact};

#[derive(Debug, Clone)]
pub struct HttpRuntimeConfig {
    pub host: String,
    pub port: u16,
    pub route_name: String,
    pub isolate_pool_size: usize,
    pub project_id: Option<String>,
    pub server_url: String,
    pub service_token: String,
}

#[derive(Clone)]
struct RuntimeState {
    route_name: String,
    code_version: String,
    project_id: Option<String>,
    pool: Arc<IsolatePool>,
    server_url: String,
    service_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    route: String,
    code_version: String,
}

#[derive(Debug, Deserialize)]
struct InternalResumeRequest {
    request: serde_json::Value,
    recorded_checkpoints: Vec<FetchCheckpoint>,
}

pub async fn run_http_runtime(config: HttpRuntimeConfig, artifact: RuntimeArtifact) -> Result<()> {
    let boot = boot_runtime_artifact(
        &artifact,
        ExecutionContext::with_project(artifact.code_version().to_string(), config.project_id.clone()),
    )
    .await?;

    if let Some(error) = boot.result.error.as_ref() {
        bail!("boot execution failed: {error}");
    }

    let pool = Arc::new(IsolatePool::new_with_mode(
        config.isolate_pool_size,
        artifact.clone(),
        boot.is_server_mode,
    )?);
    
    let state = RuntimeState {
        route_name: config.route_name.clone(),
        code_version: artifact.code_version().to_string(),
        project_id: config.project_id.clone(),
        pool,
        server_url: config.server_url,
        service_token: config.service_token,
    };

    let app: Router = Router::new()
        .route("/health", get(health))
        .route("/__flux_internal/resume", post(handle_internal_resume))
        .route("/{route}", post(handle_request))
        .fallback(handle_net_request)
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;

    tracing::info!(%addr, route = %config.route_name, "runtime listening");
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    Ok(())
}

async fn health(State(state): State<RuntimeState>) -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        route: state.route_name.clone(),
        code_version: state.code_version.clone(),
    })
}

async fn handle_request(
    State(state): State<RuntimeState>,
    Path(route): Path<String>,
    headers: axum::http::HeaderMap,
    Json(mut payload): Json<serde_json::Value>,
) -> Response {
    let request_payload = payload.clone();
    
    let provided_artifact: Option<RuntimeArtifact> = payload.get("artifact").and_then(|v| {
        serde_json::from_value::<shared::project::FluxBuildArtifact>(v.clone()).ok().map(RuntimeArtifact::Built)
    });
    
    // Only enforce route-name matching when no per-request artifact is supplied.
    // In gateway mode (--gateway) the route is always "_gateway" but callers hit /<function_name>.
    if provided_artifact.is_none() && route != state.route_name {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "route not found" })),
        )
            .into_response();
    }
    
    if let serde_json::Value::Object(ref mut map) = payload {
        map.remove("artifact");
    }

    let mut ctx = ExecutionContext::with_project(state.code_version.clone(), state.project_id.clone());
    
    let exec_id = match headers.get("x-flux-execution-id").and_then(|h| h.to_str().ok()) {
        Some(id) => id.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "x-flux-execution-id header required" })),
            ).into_response();
        }
    };
    
    ctx.execution_id = exec_id;
    
    let max_duration_ms = headers.get("x-flux-max-duration-ms")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let result = if let Some(artifact) = provided_artifact {
        // Cloud gateway mode: pass rich ctx object to handler
        ctx.cloud_ctx = true;
        execute_one_shot_artifact(artifact, payload, ctx, max_duration_ms).await
    } else {
        state.pool.execute(payload, ctx, max_duration_ms).await
    };

    if !state.service_token.is_empty() {
        let _ = crate::server_client::record_execution(
            &state.server_url,
            &state.service_token,
            crate::server_client::ExecutionEnvelope {
                method: "POST".to_string(),
                path: format!("/{}", route),
                project_id: state.project_id.clone(),
                request_json: request_payload,
                result: result.clone(),
            },
        )
        .await;
    }

    let status = if result.status == "ok" { StatusCode::OK } else { StatusCode::BAD_REQUEST };
    let mut response = (
        status,
        Json(serde_json::json!({
            "execution_id": result.execution_id,
            "status": result.status,
            "result": result.body,
            "error": result.error,
        })),
    )
        .into_response();

    attach_execution_headers(&mut response, &result.execution_id, &result.request_id, &result.code_version);
    response
}

async fn handle_net_request(
    OriginalUri(uri): OriginalUri,
    State(state): State<RuntimeState>,
    request: axum::extract::Request,
) -> Response {
    let method = request.method().to_string();

    let host = request
        .headers()
        .get("host")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    let url = format!(
        "http://{}{}",
        host,
        uri.path_and_query().map(|pq| pq.as_str()).unwrap_or("")
    );

    let headers_list: Vec<[String; 2]> = request
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            let name = k.as_str();
            if matches!(name, "x-service-token" | "x-internal-token") {
                return None;
            }
            Some([name.to_string(), v.to_str().ok()?.to_string()])
        })
        .collect();
    let headers_json = serde_json::to_string(&headers_list).unwrap_or_else(|_| "[]".to_string());

    let max_duration_ms = request.headers().get("x-flux-max-duration-ms")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let body_bytes = match to_bytes(request.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::PAYLOAD_TOO_LARGE, "request body too large").into_response();
        }
    };
    let body = if let Ok(text) = String::from_utf8(body_bytes.to_vec()) {
        text
    } else {
        format!("__FLUX_B64:{}", BASE64_STANDARD.encode(&body_bytes))
    };

    let request_payload = serde_json::json!({
        "method": method,
        "url": url,
        "headers": headers_list,
        "body": body,
    });

    let context = ExecutionContext::with_project(state.code_version.clone(), state.project_id.clone());
    let net_req = NetRequest {
        req_id: context.request_id.clone(),
        method,
        url,
        headers_json,
        body,
    };


    let result = state.pool.execute_net_request(context, net_req, max_duration_ms).await;

    if !state.service_token.is_empty() {
        let _ = crate::server_client::record_execution(
            &state.server_url,
            &state.service_token,
            crate::server_client::ExecutionEnvelope {
                method: "REPLAY".to_string(), 
                path: uri.path().to_string(),
                project_id: state.project_id.clone(),
                request_json: request_payload,
                result: result.clone(),
            },
        )
        .await;
    }

    if let Some(nr) = result.body.get("net_response") {
        let status_code = nr.get("status").and_then(|v| v.as_u64()).unwrap_or(200) as u16;
        let body_str = nr.get("body").and_then(|v| v.as_str()).unwrap_or("");
        let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        
        let mut response = if body_str.starts_with("__FLUX_B64:") {
            BASE64_STANDARD.decode(&body_str[11..]).unwrap_or_else(|_| body_str.as_bytes().to_vec()).into_response()
        } else {
            body_str.as_bytes().to_vec().into_response()
        };

        *response.status_mut() = status;
        attach_execution_headers(&mut response, &result.execution_id, &result.request_id, &result.code_version);
        response
    } else {
        let mut response = (StatusCode::INTERNAL_SERVER_ERROR, result.error.unwrap_or_else(|| "Internal error".to_string())).into_response();
        attach_execution_headers(&mut response, &result.execution_id, &result.request_id, &result.code_version);
        response
    }
}

async fn handle_internal_resume(
    State(state): State<RuntimeState>,
    headers: axum::http::HeaderMap,
    Json(_payload): Json<InternalResumeRequest>,
) -> Response {
    if state.service_token.is_empty() {
        return (StatusCode::FORBIDDEN, "runtime internal resume is disabled").into_response();
    }

    let provided = headers.get("x-internal-token").and_then(|v| v.to_str().ok()).unwrap_or_default();
    if provided != state.service_token {
        return (StatusCode::UNAUTHORIZED, "invalid internal token").into_response();
    }

    (StatusCode::OK, Json(serde_json::json!({ "status": "resume-stub" }))).into_response()
}

fn attach_execution_headers(
    response: &mut Response,
    execution_id: &str,
    request_id: &str,
    code_version: &str,
) {
    let headers = response.headers_mut();
    if let Ok(v) = HeaderValue::from_str(execution_id) {
        headers.insert(HeaderName::from_static("x-flux-execution-id"), v);
    }
    if let Ok(v) = HeaderValue::from_str(request_id) {
        headers.insert(HeaderName::from_static("x-flux-request-id"), v);
    }
    if let Ok(v) = HeaderValue::from_str(code_version) {
        headers.insert(HeaderName::from_static("x-flux-code-version"), v);
    }
}

fn error_result(ctx: ExecutionContext, err: String) -> ExecutionResult {
    ExecutionResult {
        execution_id: ctx.execution_id,
        request_id: ctx.request_id,
        project_id: ctx.project_id,
        code_version: ctx.code_version,
        status: "error".to_string(),
        body: serde_json::Value::Null,
        error: Some(err),
        duration_ms: 0,
        checkpoints: vec![],
        logs: vec![],
        has_live_io: false,
        boundary_stop: None,
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
}
