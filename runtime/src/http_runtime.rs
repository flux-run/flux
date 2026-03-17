use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Result, bail};
use axum::body::to_bytes;
use axum::extract::{OriginalUri, Path, State};
use axum::http::{HeaderName, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::artifact::RuntimeArtifact;
use crate::deno_runtime::NetRequest;
use crate::isolate_pool::{ExecutionContext, IsolatePool};

#[derive(Debug, Clone)]
pub struct HttpRuntimeConfig {
    pub host: String,
    pub port: u16,
    pub route_name: String,
    pub isolate_pool_size: usize,
    pub server_url: String,
    pub service_token: String,
}

impl Default for HttpRuntimeConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
            route_name: "hello".to_string(),
            isolate_pool_size: 16,
            server_url: "http://127.0.0.1:50051".to_string(),
            service_token: String::new(),
        }
    }
}

#[derive(Clone)]
struct RuntimeState {
    route_name: String,
    code_version: String,
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

fn attach_execution_headers<T>(response: &mut Response<T>, execution_id: &str, request_id: &str, code_version: &str) {
    if let Ok(value) = HeaderValue::from_str(execution_id) {
        response.headers_mut().insert(HeaderName::from_static("x-flux-execution-id"), value);
    }
    if let Ok(value) = HeaderValue::from_str(request_id) {
        response.headers_mut().insert(HeaderName::from_static("x-flux-request-id"), value);
    }
    if let Ok(value) = HeaderValue::from_str(code_version) {
        response.headers_mut().insert(HeaderName::from_static("x-flux-code-version"), value);
    }
}

pub async fn run_http_runtime(config: HttpRuntimeConfig, artifact: RuntimeArtifact) -> Result<()> {
    if config.isolate_pool_size == 0 {
        bail!("isolate_pool_size must be greater than 0");
    }

    let pool = Arc::new(IsolatePool::new(config.isolate_pool_size, artifact.clone())?);
    let is_server_mode = pool.is_server_mode;
    let state = RuntimeState {
        route_name: config.route_name.clone(),
        code_version: artifact.code_version().to_string(),
        pool,
        server_url: config.server_url,
        service_token: config.service_token,
    };

    let app: Router = if is_server_mode {
        // Server-mode: a fallback catches every method + path not taken by
        // the health check, and feeds it into the Deno.serve handler.
        tracing::info!("server mode detected — routing all traffic through Deno.serve handler");
        Router::new()
            .route("/health", get(health))
            .fallback(handle_net_request)
            .with_state(state)
    } else {
        Router::new()
            .route("/health", get(health))
            .route("/:route", post(handle_request))
            .with_state(state)
    };

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
        route: state.route_name,
        code_version: state.code_version,
    })
}

/// One-shot handler: POST /:route — runs the exported default handler function.
async fn handle_request(
    Path(route): Path<String>,
    State(state): State<RuntimeState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    if route != state.route_name {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "route not found" })),
        )
            .into_response();
    }

    let request_payload = payload.clone();
    let result = state
        .pool
        .execute(payload, ExecutionContext::new(state.code_version.clone()))
        .await;

    if !state.service_token.is_empty() {
        let _ = crate::server_client::record_execution(
            &state.server_url,
            &state.service_token,
            crate::server_client::ExecutionEnvelope {
                method: "POST".to_string(),
                path: format!("/{}", route),
                request_json: request_payload,
                result: result.clone(),
            },
        )
        .await;
    }

    if result.status != "ok" {
        let mut response = (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "execution_id": result.execution_id,
                "request_id": result.request_id,
                "code_version": result.code_version,
                "status": result.status,
                "error": result.error,
            })),
        )
            .into_response();
        attach_execution_headers(&mut response, &result.execution_id, &result.request_id, &result.code_version);
        return response;
    }

    let mut response = (
        StatusCode::OK,
        Json(serde_json::json!({
            "execution_id": result.execution_id,
            "request_id": result.request_id,
            "code_version": result.code_version,
            "status": result.status,
            "result": result.body,
            "error": result.error,
        })),
    )
        .into_response();
    attach_execution_headers(&mut response, &result.execution_id, &result.request_id, &result.code_version);
    response
}

/// Server-mode handler: any method, any path — dispatches through Deno.serve.
async fn handle_net_request(
    OriginalUri(uri): OriginalUri,
    State(state): State<RuntimeState>,
    request: axum::extract::Request,
) -> impl IntoResponse {
    let method = request.method().to_string();

    // Build the absolute URL the JS handler will see.
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

    // Collect non-sensitive headers as [[name, value], ...] JSON.
    let headers_list: Vec<[String; 2]> = request
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            let name = k.as_str();
            // Never forward auth / internal tokens into user code.
            if matches!(name, "authorization" | "x-service-token" | "x-internal-token") {
                return None;
            }
            Some([name.to_string(), v.to_str().ok()?.to_string()])
        })
        .collect();
    let headers_json = serde_json::to_string(&headers_list).unwrap_or_else(|_| "[]".to_string());

    // Read body (cap at 10 MB).
    let body_bytes = match to_bytes(request.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (StatusCode::PAYLOAD_TOO_LARGE, "request body too large").into_response();
        }
    };
    let body = String::from_utf8_lossy(&body_bytes).into_owned();

    let request_payload = serde_json::json!({
        "method": method,
        "url": url,
        "headers": headers_list,
        "body": body,
    });

    let req_id = Uuid::new_v4().to_string();
    let net_req = NetRequest {
        req_id,
        method: request_payload["method"].as_str().unwrap_or("GET").to_string(),
        url: request_payload["url"].as_str().unwrap_or_default().to_string(),
        headers_json,
        body: request_payload["body"].as_str().unwrap_or_default().to_string(),
    };
    let context = ExecutionContext::new(state.code_version.clone());
    let result = state.pool.execute_net_request(context, net_req).await;

    if !state.service_token.is_empty() {
        let _ = crate::server_client::record_execution(
            &state.server_url,
            &state.service_token,
            crate::server_client::ExecutionEnvelope {
                method: request_payload["method"].as_str().unwrap_or("GET").to_string(),
                path: uri
                    .path_and_query()
                    .map(|value| value.as_str().to_string())
                    .unwrap_or_else(|| uri.path().to_string()),
                request_json: request_payload.clone(),
                result: result.clone(),
            },
        )
        .await;
    }

    if let Some(err) = &result.error {
        let mut response = (StatusCode::INTERNAL_SERVER_ERROR, err.clone()).into_response();
        attach_execution_headers(&mut response, &result.execution_id, &result.request_id, &result.code_version);
        return response;
    }

    // Unpack the net_response envelope written by the worker.
    if let Some(nr) = result.body.get("net_response") {
        let status_code = nr.get("status").and_then(|v| v.as_u64()).unwrap_or(200) as u16;
        let body_str = nr.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let raw_headers = nr.get("headers").and_then(|v| v.as_array()).cloned().unwrap_or_default();

        let status = StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
        let mut response = Response::new(body_str);
        *response.status_mut() = status;

        for entry in &raw_headers {
            if let Some(arr) = entry.as_array() {
                if arr.len() == 2 {
                    let k = arr[0].as_str().unwrap_or("");
                    let v = arr[1].as_str().unwrap_or("");
                    if let (Ok(name), Ok(value)) = (
                        k.parse::<HeaderName>(),
                        v.parse::<HeaderValue>(),
                    ) {
                        response.headers_mut().insert(name, value);
                    }
                }
            }
        }

        attach_execution_headers(&mut response, &result.execution_id, &result.request_id, &result.code_version);

        return response.into_response();
    }

    let mut response = (StatusCode::INTERNAL_SERVER_ERROR, "handler produced no response").into_response();
    attach_execution_headers(&mut response, &result.execution_id, &result.request_id, &result.code_version);
    response
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    {
        let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = ctrl_c => {}
            _ = terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await;
}
