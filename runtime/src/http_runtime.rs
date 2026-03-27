use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{bail, Result};
use axum::body::to_bytes;
use axum::extract::{OriginalUri, Path, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::artifact::RuntimeArtifact;
use crate::deno_runtime::{boot_runtime_artifact, FetchCheckpoint, NetRequest};
use crate::isolate_pool::{execute_one_shot_artifact, ExecutionContext, IsolatePool};

const EXECUTOR_RECORDING_OWNER_HEADER: &str = "x-flux-recording-owner";
const EXECUTOR_RECORDING_OWNER_VALUE: &str = "executor";

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

/// Shared runtime state — the pool is behind a RwLock so it can be hot-swapped
/// on every `flux deploy` without restarting the process.
#[derive(Clone)]
struct RuntimeState {
    route_name: String,
    code_version: Arc<RwLock<String>>,
    isolate_pool_size: usize,
    project_id: Option<String>,
    pool: Arc<RwLock<IsolatePool>>,
    server_url: String,
    service_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    route: String,
    code_version: String,
    is_server_mode: bool,
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct InternalResumeRequest {
    request: serde_json::Value,
    recorded_checkpoints: Vec<FetchCheckpoint>,
}

/// Request body for the hot-reload endpoint.
/// `artifact` is the full FluxBuildArtifact JSON, as returned by `flux build`.
#[derive(Debug, Deserialize)]
struct InternalReloadRequest {
    artifact: serde_json::Value,
}

pub async fn run_http_runtime(config: HttpRuntimeConfig, artifact: RuntimeArtifact) -> Result<()> {
    // Determine is_server_mode by booting once (run_http_runtime is called from main, so Send isn't an issue here)
    let boot = boot_runtime_artifact(
        &artifact,
        ExecutionContext::with_project(
            artifact.code_version().to_string(),
            config.project_id.clone(),
        ),
    )
    .await?;

    if let Some(error) = boot.result.error.as_ref() {
        bail!("boot execution failed: {error}");
    }

    let pool = Arc::new(RwLock::new(IsolatePool::new_with_mode(
        config.isolate_pool_size,
        artifact.clone(),
        boot.is_server_mode,
    )?));

    let state = RuntimeState {
        route_name: config.route_name.clone(),
        code_version: Arc::new(RwLock::new(artifact.code_version().to_string())),
        isolate_pool_size: config.isolate_pool_size,
        project_id: config.project_id.clone(),
        pool,
        server_url: config.server_url,
        service_token: config.service_token,
    };

    let app: Router = Router::new()
        .route("/health", get(health))
        .route("/__flux_internal/resume", post(handle_internal_resume))
        .route("/__flux_internal/reload", post(handle_internal_reload))
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
    let pool = state.pool.read().await;
    let code_version = state.code_version.read().await.clone();
    Json(HealthResponse {
        status: "ok".to_string(),
        route: state.route_name.clone(),
        code_version,
        is_server_mode: pool.is_server_mode,
    })
}

fn gateway_mode_unavailable_response() -> Response {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "error": "no_artifact_loaded",
            "message": "The runtime is in gateway mode and cannot serve public traffic directly.",
            "tip": "Route public traffic through flux-executor, or hot-swap/provide an artifact before invoking the runtime."
        })),
    )
        .into_response()
}

fn runtime_should_record_execution(
    state: &RuntimeState,
    headers: &axum::http::HeaderMap,
) -> bool {
    if state.service_token.is_empty() {
        return false;
    }

    headers
        .get(EXECUTOR_RECORDING_OWNER_HEADER)
        .and_then(|value| value.to_str().ok())
        != Some(EXECUTOR_RECORDING_OWNER_VALUE)
}

async fn handle_request(
    State(state): State<RuntimeState>,
    Path(route): Path<String>,
    headers: axum::http::HeaderMap,
    Json(mut payload): Json<serde_json::Value>,
) -> Response {
    let request_payload = payload.clone();

    let provided_artifact: Option<RuntimeArtifact> = payload.get("artifact").and_then(|v| {
        serde_json::from_value::<shared::project::FluxBuildArtifact>(v.clone())
            .ok()
            .map(RuntimeArtifact::Built)
    });

    if provided_artifact.is_none() && state.route_name == "_gateway" {
        return gateway_mode_unavailable_response();
    }

    // Only enforce route-name matching when no per-request artifact is supplied.
    if provided_artifact.is_none() && route != state.route_name && state.route_name != "_gateway" {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "route not found" })),
        )
            .into_response();
    }

    if let serde_json::Value::Object(ref mut map) = payload {
        map.remove("artifact");
    }

    let code_version = state.code_version.read().await.clone();
    let mut ctx = ExecutionContext::with_project(code_version.clone(), state.project_id.clone());

    let exec_id = match headers
        .get("x-flux-execution-id")
        .and_then(|h| h.to_str().ok())
    {
        Some(id) => id.to_string(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "x-flux-execution-id header required" })),
            )
                .into_response();
        }
    };

    ctx.execution_id = exec_id;

    let max_duration_ms = headers
        .get("x-flux-max-duration-ms")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());

    let result = if let Some(artifact) = provided_artifact {
        let pool = state.pool.read().await;
        if pool.artifact_id() == artifact.code_version() {
            // Already warm! Use the pool.
            pool.execute(payload, ctx, max_duration_ms).await
        } else {
            // Cold start fallback
            ctx.cloud_ctx = true;
            execute_one_shot_artifact(artifact, payload, ctx, max_duration_ms).await
        }
    } else {
        // Shared global runtime mode: use whatever is currently warm
        let pool = state.pool.read().await;
        pool.execute(payload, ctx, max_duration_ms).await
    };

    if runtime_should_record_execution(&state, &headers) {
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

    let status = if result.status == "ok" {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    };
    let mut response = (
        status,
        Json(serde_json::json!({
            "execution_id": result.execution_id,
            "status": result.status,
            "result": result.body,
            "error": result.error,
            "error_name": result.error_name,
            "error_message": result.error_message,
            "error_stack": result.error_stack,
            "error_phase": result.error_phase,
            "is_user_code": result.is_user_code,
            "error_source": result.error_source,
            "error_type": result.error_type,
            "duration_ms": result.duration_ms,
            "checkpoints": result.checkpoints,
            "logs": result.logs,
        })),
    )
        .into_response();

    let code_v = state.code_version.read().await.clone();
    attach_execution_headers(
        &mut response,
        &result.execution_id,
        &result.request_id,
        &code_v,
    );
    response
}

async fn handle_net_request(
    OriginalUri(uri): OriginalUri,
    State(state): State<RuntimeState>,
    request: axum::extract::Request,
) -> Response {
    if state.route_name == "_gateway" {
        return gateway_mode_unavailable_response();
    }

    let should_record_execution = runtime_should_record_execution(&state, request.headers());
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
            if matches!(
                name,
                "x-service-token"
                    | "x-internal-token"
                    | "x-flux-execution-id"
                    | "x-flux-project-id"
                    | "x-flux-max-duration-ms"
                    | EXECUTOR_RECORDING_OWNER_HEADER
            ) {
                return None;
            }
            Some([name.to_string(), v.to_str().ok()?.to_string()])
        })
        .collect();
    let headers_json = serde_json::to_string(&headers_list).unwrap_or_else(|_| "[]".to_string());

    let max_duration_ms = request
        .headers()
        .get("x-flux-max-duration-ms")
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

    let code_version = state.code_version.read().await.clone();
    let context = ExecutionContext::with_project(code_version.clone(), state.project_id.clone());
    let net_req = NetRequest {
        req_id: context.request_id.clone(),
        method,
        url,
        headers_json,
        body,
    };

    let result = {
        let pool = state.pool.read().await;
        pool.execute_net_request(context, net_req, max_duration_ms)
            .await
    };

    if should_record_execution {
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
            BASE64_STANDARD
                .decode(&body_str[11..])
                .unwrap_or_else(|_| body_str.as_bytes().to_vec())
                .into_response()
        } else {
            body_str.as_bytes().to_vec().into_response()
        };

        *response.status_mut() = status;
        let code_v = state.code_version.read().await.clone();
        attach_execution_headers(
            &mut response,
            &result.execution_id,
            &result.request_id,
            &code_v,
        );
        response
    } else {
        let mut response = (
            StatusCode::INTERNAL_SERVER_ERROR,
            result.error.unwrap_or_else(|| "Internal error".to_string()),
        )
            .into_response();
        let code_v = state.code_version.read().await.clone();
        attach_execution_headers(
            &mut response,
            &result.execution_id,
            &result.request_id,
            &code_v,
        );
        response
    }
}

/// Hot-swap the isolate pool with a new artifact, without restarting the process.
/// Called by flux-executor immediately after detecting a deployment update.
async fn handle_internal_reload(
    State(state): State<RuntimeState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<InternalReloadRequest>,
) -> Response {
    if state.service_token.is_empty() {
        tracing::error!("❌ Hot-reload rejected: service_token is empty");
        return (StatusCode::FORBIDDEN, "runtime internal reload is disabled").into_response();
    }
    let provided = headers
        .get("x-internal-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if provided != state.service_token {
        tracing::error!(
            "❌ Hot-reload rejected: invalid internal token. Provided: '{}', Expected: '{}'",
            provided,
            state.service_token
        );
        return (StatusCode::UNAUTHORIZED, "invalid internal token").into_response();
    }

    tracing::info!("🔄 Hot-reload request received");

    tracing::info!("🔍 Parsing artifact JSON...");
    let new_artifact =
        match serde_json::from_value::<shared::project::FluxBuildArtifact>(payload.artifact) {
            Ok(a) => RuntimeArtifact::Built(a),
            Err(e) => {
                tracing::error!("❌ Artifact parse error: {}", e);
                return (StatusCode::BAD_REQUEST, format!("invalid artifact: {}", e))
                    .into_response();
            }
        };

    let new_version = new_artifact.code_version().to_string();
    tracing::info!("📖 Acquiring code_version read lock...");
    let current_version = state.code_version.read().await.clone();
    tracing::info!(
        "🎯 Comparison: new='{}', current='{}'",
        new_version,
        current_version
    );

    if new_version == current_version {
        tracing::info!("⏭️ Skipping reload: version already matches");
        return (
            StatusCode::OK,
            Json(serde_json::json!({ "reloaded": false, "reason": "Already on this version" })),
        )
            .into_response();
    }

    tracing::info!(
        "🔄 Hot-reloading runtime with artifact version: {}",
        new_version
    );

    // DETERMINISTIC THREAD-OFF: boot_runtime_artifact is !Send because it creates a JsRuntime.
    // We run it in a separate thread and signal result back to this sync/Send-safe handler.
    let (tx, rx) = tokio::sync::oneshot::channel();
    let ctx = ExecutionContext::with_project(new_version.clone(), state.project_id.clone());

    tracing::info!("🧵 Spawning reload worker thread...");
    std::thread::spawn(move || {
        tracing::info!("👷 Worker thread started. Building local tokio runtime...");
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build local reload runtime");

        tracing::info!("🚀 Booting runtime artifact in worker thread...");
        let result = rt.block_on(async { boot_runtime_artifact(&new_artifact, ctx).await });
        if let Err(ref e) = result {
            tracing::error!("❌ Boot artifact failed in worker thread: {}", e);
        }
        tracing::info!("👢 Boot complete. Success? {}", result.is_ok());
        let _ = tx.send((result, new_artifact));
    });

    tracing::info!("⏳ Waiting for boot results from worker thread...");
    let (boot_result, artifact) = match rx.await {
        Ok(res) => {
            tracing::info!("📥 Received results from worker thread");
            res
        }
        Err(_) => {
            tracing::error!("❌ Reload worker thread panicked or dropped sender");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Reload worker thread panicked",
            )
                .into_response();
        }
    };

    let boot = match boot_result {
        Ok(b) => b,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("boot failed: {}", e),
            )
                .into_response()
        }
    };

    if let Some(error) = boot.result.error.as_ref() {
        tracing::error!("❌ Boot script error: {}", error);
        return (
            StatusCode::BAD_REQUEST,
            format!("boot execution error: {}", error),
        )
            .into_response();
    }

    // Allocate the new pool based on the boot detection
    let new_pool =
        match IsolatePool::new_with_mode(state.isolate_pool_size, artifact, boot.is_server_mode) {
            Ok(p) => p,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("pool allocation failed: {}", e),
                )
                    .into_response()
            }
        };

    // Atomic swap
    *state.pool.write().await = new_pool;
    *state.code_version.write().await = new_version.clone();

    tracing::info!("✅ Runtime hot-reloaded to version: {}", new_version);
    (
        StatusCode::OK,
        Json(serde_json::json!({ "reloaded": true, "code_version": new_version })),
    )
        .into_response()
}

async fn handle_internal_resume(
    State(state): State<RuntimeState>,
    headers: axum::http::HeaderMap,
    Json(_payload): Json<InternalResumeRequest>,
) -> Response {
    if state.service_token.is_empty() {
        return (StatusCode::FORBIDDEN, "runtime internal resume is disabled").into_response();
    }

    let provided = headers
        .get("x-internal-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if provided != state.service_token {
        return (StatusCode::UNAUTHORIZED, "invalid internal token").into_response();
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "status": "resume-stub" })),
    )
        .into_response()
}

fn attach_execution_headers(
    response: &mut Response,
    execution_id: &str,
    request_id: &str,
    code_version: &str,
) {
    let headers = response.headers_mut();
    let _ = HeaderValue::from_str(execution_id)
        .map(|v| headers.insert(HeaderName::from_static("x-flux-execution-id"), v));
    let _ = HeaderValue::from_str(request_id)
        .map(|v| headers.insert(HeaderName::from_static("x-flux-request-id"), v));
    let _ = HeaderValue::from_str(code_version)
        .map(|v| headers.insert(HeaderName::from_static("x-flux-code-version"), v));
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c().await.ok();
}
