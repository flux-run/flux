use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Result, bail};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};

use crate::artifact::RuntimeArtifact;
use crate::isolate_pool::{ExecutionContext, IsolatePool};

#[derive(Debug, Clone)]
pub struct HttpRuntimeConfig {
    pub host: String,
    pub port: u16,
    pub route_name: String,
    pub isolate_pool_size: usize,
}

impl Default for HttpRuntimeConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 3000,
            route_name: "hello".to_string(),
            isolate_pool_size: 16,
        }
    }
}

#[derive(Clone)]
struct RuntimeState {
    route_name: String,
    code_version: String,
    pool: Arc<IsolatePool>,
}

#[derive(Debug, Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    route: String,
    code_version: String,
}

pub async fn run_http_runtime(config: HttpRuntimeConfig, artifact: RuntimeArtifact) -> Result<()> {
    if config.isolate_pool_size == 0 {
        bail!("isolate_pool_size must be greater than 0");
    }

    let state = RuntimeState {
        route_name: config.route_name.clone(),
        code_version: artifact.sha256.clone(),
        pool: Arc::new(IsolatePool::new(config.isolate_pool_size, &artifact.code)?),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/:route", post(handle_request))
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
        route: state.route_name,
        code_version: state.code_version,
    })
}

async fn handle_request(
    Path(route): Path<String>,
    State(state): State<RuntimeState>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    if route != state.route_name {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "route not found",
            })),
        )
            .into_response();
    }

    let mut isolate = match state.pool.acquire().await {
        Ok(isolate) => isolate,
        Err(err) => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": format!("failed to acquire isolate: {err}"),
                })),
            )
                .into_response()
        }
    };

    isolate.set_context(ExecutionContext::new(state.code_version.clone()));
    let result = isolate.run(payload, &route).await;

    if result.status != "ok" {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "request_id": result.request_id,
                "code_version": result.code_version,
                "status": result.status,
                "error": result.error,
            })),
        )
            .into_response();
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "request_id": result.request_id,
            "code_version": result.code_version,
            "status": result.status,
            "result": result.body,
            "error": result.error,
        })),
    )
        .into_response()
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
