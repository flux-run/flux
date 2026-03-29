use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
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
use tokio::time::Duration;

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
    /// True once the pool has been loaded with a real user artifact (either at startup
    /// or via hot-reload). In gateway mode this starts `false` and flips to `true` on
    /// the first successful `/__flux_internal/reload`.
    loaded: Arc<AtomicBool>,
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

    // Gateway starts unloaded; any other launch (--artifact / --entry) is already loaded.
    let is_gateway = config.route_name == "_gateway";
    let state = RuntimeState {
        route_name: config.route_name.clone(),
        code_version: Arc::new(RwLock::new(artifact.code_version().to_string())),
        isolate_pool_size: config.isolate_pool_size,
        project_id: config.project_id.clone(),
        pool,
        server_url: config.server_url,
        service_token: config.service_token,
        loaded: Arc::new(AtomicBool::new(!is_gateway)),
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

fn request_originates_from_executor(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(EXECUTOR_RECORDING_OWNER_HEADER)
        .and_then(|value| value.to_str().ok())
        == Some(EXECUTOR_RECORDING_OWNER_VALUE)
}

fn runtime_should_record_execution(
    state: &RuntimeState,
    headers: &axum::http::HeaderMap,
) -> bool {
    if state.service_token.is_empty() {
        return false;
    }

    !request_originates_from_executor(headers)
}

fn cached_execution_type(hit: &crate::server_client::CompletedExecutionCacheHit) -> &'static str {
    match (
        !hit.error.is_empty() || !hit.error_source.is_empty() || !hit.error_type.is_empty(),
        hit.error_source.as_str(),
    ) {
        (false, _) => "success",
        (true, "platform_runtime") => "infra_error",
        (true, _) => "user_error",
    }
}

fn cached_invoke_payload(
    hit: &crate::server_client::CompletedExecutionCacheHit,
) -> serde_json::Value {
    if let serde_json::Value::Object(mut map) = hit.response_json.clone() {
        if map.contains_key("execution_id") {
            map.insert("idempotent_hit".to_string(), serde_json::Value::Bool(true));
            map.insert(
                "cached_attempt".to_string(),
                serde_json::Value::Number(hit.attempt.into()),
            );
            return serde_json::Value::Object(map);
        }
    }

    serde_json::json!({
        "execution_id": hit.execution_id,
        "status": hit.status,
        "type": cached_execution_type(hit),
        "result": hit.response_json,
        "error": if hit.error.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(hit.error.clone()) },
        "error_name": if hit.error_name.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(hit.error_name.clone()) },
        "error_message": if hit.error_message.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(hit.error_message.clone()) },
        "error_stack": if hit.error_stack.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(hit.error_stack.clone()) },
        "error_frames": hit.error_frames,
        "error_phase": if hit.error_phase.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(hit.error_phase.clone()) },
        "is_user_code": hit.is_user_code,
        "error_source": if hit.error_source.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(hit.error_source.clone()) },
        "error_type": if hit.error_type.is_empty() { serde_json::Value::Null } else { serde_json::Value::String(hit.error_type.clone()) },
        "duration_ms": hit.duration_ms,
        "checkpoints": [],
        "logs": [],
        "idempotent_hit": true,
        "cached_attempt": hit.attempt,
    })
}

fn cached_response_from_net_response(
    net_response: &serde_json::Value,
    hit: &crate::server_client::CompletedExecutionCacheHit,
) -> Response {
    let status_code = net_response
        .get("status")
        .and_then(|value| value.as_u64())
        .unwrap_or(hit.response_status as u64) as u16;
    let body_str = net_response
        .get("body")
        .and_then(|value| value.as_str())
        .unwrap_or(&hit.response_body);
    let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::OK);

    let mut response = if let Some(encoded) = body_str.strip_prefix("__FLUX_B64:") {
        BASE64_STANDARD
            .decode(encoded)
            .unwrap_or_else(|_| body_str.as_bytes().to_vec())
            .into_response()
    } else {
        body_str.as_bytes().to_vec().into_response()
    };

    *response.status_mut() = status;
    if let Some(headers) = net_response.get("headers").and_then(|value| value.as_array()) {
        for pair in headers {
            let name = pair.get(0).and_then(|value| value.as_str());
            let value = pair.get(1).and_then(|value| value.as_str());
            if let (Some(name), Some(value)) = (name, value) {
                if let (Ok(header_name), Ok(header_value)) = (
                    HeaderName::from_bytes(name.as_bytes()),
                    HeaderValue::from_str(value),
                ) {
                    response.headers_mut().insert(header_name, header_value);
                }
            }
        }
    }

    response
}

async fn maybe_get_completed_execution_hit(
    state: &RuntimeState,
    request_id: &str,
) -> Option<crate::server_client::CompletedExecutionCacheHit> {
    if state.service_token.is_empty() {
        return None;
    }

    let fut = crate::server_client::get_completed_execution_by_request(
        &state.server_url,
        &state.service_token,
        request_id,
    );
    match tokio::time::timeout(Duration::from_millis(500), fut).await {
        Ok(Ok(hit)) => hit,
        Ok(Err(error)) => {
            tracing::warn!(request_id = %request_id, error = %error, "completed-execution lookup failed");
            None
        }
        Err(_) => {
            tracing::warn!(request_id = %request_id, "completed-execution lookup timed out, proceeding");
            None
        }
    }
}

/// Attempt to atomically claim the execution slot for (request_id, attempt=1).
/// Returns `true` if this runtime won the race, `false` if another already claimed it.
/// On network failure, returns `true` (proceed rather than stall).
async fn try_claim_execution(state: &RuntimeState, execution_id: &str, request_id: &str) -> bool {
    if state.service_token.is_empty() || state.server_url.is_empty() {
        return true;
    }
    let fut = crate::server_client::claim_execution(
        &state.server_url,
        &state.service_token,
        execution_id,
        request_id,
        1,
    );
    match tokio::time::timeout(Duration::from_millis(500), fut).await {
        Ok(Ok(claimed)) => claimed,
        Ok(Err(error)) => {
            tracing::warn!(execution_id = %execution_id, request_id = %request_id, error = %error, "claim_execution failed, proceeding");
            true
        }
        Err(_) => {
            tracing::warn!(execution_id = %execution_id, request_id = %request_id, "claim_execution timed out, proceeding");
            true
        }
    }
}

async fn handle_request(
    State(state): State<RuntimeState>,
    Path(route): Path<String>,
    headers: axum::http::HeaderMap,
    Json(mut payload): Json<serde_json::Value>,
) -> Response {
    let request_payload = payload.clone();

    let provided_artifact: Option<RuntimeArtifact> = payload.get("artifact").and_then(|v| {
        match serde_json::from_value::<shared::project::FluxBuildArtifact>(v.clone()) {
            Ok(a) => Some(RuntimeArtifact::Built(a)),
            Err(e) => {
                tracing::warn!(
                    route = %route,
                    err = %e,
                    "artifact deserialization failed — treating as no artifact"
                );
                None
            }
        }
    });

    // Block requests only when the gateway has never been loaded with real user code.
    // After a successful hot-reload `state.loaded` is true even though route_name is
    // still "_gateway", so requests can be served from the warm pool.
    if provided_artifact.is_none()
        && state.route_name == "_gateway"
        && !state.loaded.load(Ordering::Acquire)
    {
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
    if let Some(request_id) = headers
        .get("x-flux-request-id")
        .and_then(|h| h.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        ctx.request_id = request_id.to_string();
    }
    if request_originates_from_executor(&headers) {
        ctx.cloud_ctx = true;
    }

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

    if let Some(hit) = maybe_get_completed_execution_hit(&state, &ctx.request_id).await {
        let status = if hit.status == "ok" {
            StatusCode::OK
        } else {
            StatusCode::BAD_REQUEST
        };
        let mut response = (status, Json(cached_invoke_payload(&hit))).into_response();
        attach_execution_headers(
            &mut response,
            &hit.execution_id,
            &hit.request_id,
            &hit.code_version,
        );
        return response;
    }

    if !try_claim_execution(&state, &ctx.execution_id, &ctx.request_id).await {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "execution already in progress for this request",
                "code": "concurrent_execution"
            })),
        )
            .into_response();
    }

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
            execute_one_shot_artifact(artifact, payload, ctx, max_duration_ms).await
        }
    } else {
        // Shared global runtime mode: use whatever is currently warm
        let pool = state.pool.read().await;
        pool.execute(payload, ctx, max_duration_ms).await
    };

    tracing::info!(
        execution_id = %result.execution_id,
        request_id = %result.request_id,
        route = %route,
        status = %result.status,
        checkpoints = result.checkpoints.len(),
        logs = result.logs.len(),
        from_executor = request_originates_from_executor(&headers),
        "runtime handle_request execution complete"
    );

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
    let execution_type = match (result.error.is_some(), result.error_source.as_deref()) {
        (false, _) => "success",
        (true, Some("platform_runtime")) => "infra_error",
        (true, _) => "user_error",
    };
    let mut response = (
        status,
        Json(serde_json::json!({
            "execution_id": result.execution_id,
            "status": result.status,
            "type": execution_type,
            "result": result.body,
            "error": result.error,
            "error_name": result.error_name,
            "error_message": result.error_message,
            "error_stack": result.error_stack,
            "error_frames": result.error_frames,
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
    if state.route_name == "_gateway" && !state.loaded.load(Ordering::Acquire) {
        return gateway_mode_unavailable_response();
    }

    let request_from_executor = request_originates_from_executor(request.headers());
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

    let provided_request_id = request
        .headers()
        .get("x-flux-request-id")
        .and_then(|h| h.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

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
    let mut context = ExecutionContext::with_project(code_version.clone(), state.project_id.clone());
    if let Some(request_id) = provided_request_id {
        context.request_id = request_id;
    }
    let net_req = NetRequest {
        req_id: context.request_id.clone(),
        method,
        url,
        headers_json,
        body,
    };

    if let Some(hit) = maybe_get_completed_execution_hit(&state, &context.request_id).await {
        if request_from_executor {
            let status = if hit.status == "ok" {
                StatusCode::OK
            } else {
                StatusCode::BAD_REQUEST
            };
            let mut response = (status, Json(cached_invoke_payload(&hit))).into_response();
            attach_execution_headers(
                &mut response,
                &hit.execution_id,
                &hit.request_id,
                &hit.code_version,
            );
            return response;
        }

        let mut response = if let Some(net_response) = hit.response_json.get("net_response") {
            cached_response_from_net_response(net_response, &hit)
        } else {
            let fallback = serde_json::json!({
                "status": if hit.response_status > 0 { hit.response_status } else { 200 },
                "body": hit.response_body,
            });
            cached_response_from_net_response(&fallback, &hit)
        };
        attach_execution_headers(
            &mut response,
            &hit.execution_id,
            &hit.request_id,
            &hit.code_version,
        );
        return response;
    }

    if !try_claim_execution(&state, &context.execution_id, &context.request_id).await {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "execution already in progress for this request",
                "code": "concurrent_execution"
            })),
        )
            .into_response();
    }

    let result = {
        let pool = state.pool.read().await;
        pool.execute_net_request(context, net_req, max_duration_ms)
            .await
    };

    tracing::info!(
        execution_id = %result.execution_id,
        request_id = %result.request_id,
        path = %uri.path(),
        status = %result.status,
        checkpoints = result.checkpoints.len(),
        logs = result.logs.len(),
        from_executor = request_from_executor,
        "runtime handle_net_request execution complete"
    );

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

    if request_from_executor {
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
                "error_frames": result.error_frames,
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
    } else if let Some(nr) = result.body.get("net_response") {
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
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": format!("invalid artifact: {}", e) })),
                )
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
            Json(serde_json::json!({ "error": format!("boot execution error: {}", error) })),
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
    state.loaded.store(true, Ordering::Release);

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
