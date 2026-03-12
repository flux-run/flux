//! Axum route table.
//!
//! Three routes — exactly matching `docs/gateway.md`:
//!   GET /health    — liveness probe (always 200)
//!   GET /readiness — readiness probe (503 until snapshot loaded)
//!   ANY /{*path}   — function invocation
use axum::{routing::{any, get}, Router};
use tower_http::cors::{CorsLayer, Any};
use crate::state::SharedState;

pub fn create_router(state: SharedState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .route("/health",    get(crate::handlers::health::handle))
        .route("/readiness", get(crate::handlers::readiness::handle))
        .route("/{*path}",   any(crate::handlers::dispatch::handle))
        .layer(cors)
        .with_state(state)
}
