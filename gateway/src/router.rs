use axum::{routing::any, routing::get, routing::post, Router, response::IntoResponse};
use tower_http::cors::{CorsLayer, Any};
use crate::state::SharedState;
use crate::routes::proxy::proxy_handler;
use crate::routes::data_engine;

pub fn create_router(state: SharedState) -> Router {
    // CORS — allow all origins/methods/headers so the browser can call
    // execution routes (query, file ops) directly from localhost.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Execution-plane routes — transparent proxy to the data engine.
    // Registered BEFORE the catch-all so they take priority.
    let engine_routes = Router::new()
        .route("/db/query",    post(data_engine::proxy_handler))
        .route("/db/{*path}",    any(data_engine::proxy_handler))
        .route("/files/{*path}", any(data_engine::proxy_handler));

    // Realtime SSE — proxy to the API service.
    let event_routes = Router::new()
        .route("/events/stream", get(crate::routes::events::stream));

    // Internal management routes — service-token protected, not exposed via cors.
    let internal_routes = Router::new()
        .route("/internal/cache/invalidate", post(crate::routes::cache::invalidate_handler))
        .route("/internal/cache/stats",      get(crate::routes::cache::stats_handler));

    // Execution-plane documentation — tenant-scoped OpenAPI + Swagger UI + agent schema.
    // These must be registered BEFORE the catch-all fn_routes so they are matched first.
    let docs_routes = Router::new()
        .route("/openapi.json", axum::routing::get(crate::routes::openapi::openapi_json))
        .route("/docs",        axum::routing::get(crate::routes::openapi::docs_ui))
        .route("/agent-schema", axum::routing::get(crate::routes::openapi::agent_schema));

    // Serverless-function invocation routes — existing proxy with identity middleware.
    let fn_routes = Router::new()
        .route("/{*path}", any(proxy_handler))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::identity_resolver::resolve_identity,
        ));

    Router::new()
        .route("/health", axum::routing::get(|| async {
            axum::Json(serde_json::json!({ "status": "ok" }))
        }))
        // /readiness — returns 503 until the route snapshot is populated.
        // Distinct from /health (which is always 200) so Cloud Run and load
        // balancers can gate traffic until the gateway is actually ready to route.
        .route("/readiness", axum::routing::get(|axum::extract::State(state): axum::extract::State<SharedState>| async move {
            let data = state.snapshot.get_data().await;
            if data.routes.is_empty() {
                (
                    axum::http::StatusCode::SERVICE_UNAVAILABLE,
                    axum::Json(serde_json::json!({ "status": "loading", "message": "Route snapshot not yet loaded" })),
                ).into_response()
            } else {
                (
                    axum::http::StatusCode::OK,
                    axum::Json(serde_json::json!({ "status": "ready", "routes": data.routes.len() })),
                ).into_response()
            }
        }))
        // /metrics — lightweight process-level counters (no Prometheus dep).
        // Useful for Cloud Run sidecar scrapers or a simple healthcheck script.
        .route("/metrics", axum::routing::get(|| async {
            use crate::middleware::analytics::{DROPPED_METRICS, CHANNEL_CAPACITY};
            use std::sync::atomic::Ordering;
            axum::Json(serde_json::json!({
                "gateway_metrics_dropped_total":  DROPPED_METRICS.load(Ordering::Relaxed),
                "gateway_analytics_channel_capacity": CHANNEL_CAPACITY,
            }))
        }))
        .route("/version", axum::routing::get(|| async {
            axum::Json(serde_json::json!({
                "service": "gateway",
                "commit": std::env::var("GIT_SHA").unwrap_or_else(|_| "unknown".to_string()),
                "build_time": std::env::var("BUILD_TIME").unwrap_or_else(|_| "unknown".to_string())
            }))
        }))
        .merge(engine_routes)
        .merge(event_routes)
        .merge(internal_routes)
        .merge(docs_routes)
        .merge(fn_routes)
        .layer(cors)
        .layer(axum::extract::DefaultBodyLimit::max(1 * 1024 * 1024)) // 1 MB
        .with_state(state)
}
