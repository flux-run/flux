use axum::{routing::any, routing::get, routing::post, Router};
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
        .route("/db/{*path}",    any(data_engine::proxy_handler))
        .route("/files/{*path}", any(data_engine::proxy_handler));

    // Realtime SSE — proxy to the API service.
    let event_routes = Router::new()
        .route("/events/stream", get(crate::routes::events::stream));

    // Internal management routes — service-token protected, not exposed via cors.
    let internal_routes = Router::new()
        .route("/internal/cache/invalidate", post(crate::routes::cache::invalidate_handler))
        .route("/internal/cache/stats",      get(crate::routes::cache::stats_handler));

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
        .merge(engine_routes)
        .merge(event_routes)
        .merge(internal_routes)
        .merge(fn_routes)
        .layer(cors)
        .with_state(state)
}
