use axum::{
    routing::{any},
    Router,
};
use crate::state::SharedState;
use crate::routes::proxy::proxy_handler;

pub fn create_router(state: SharedState) -> Router {
    let api_routes = Router::new()
        .route("/{*path}", any(proxy_handler))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::identity_resolver::resolve_identity,
        ));

    Router::new()
        .route("/health", axum::routing::get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }))
        .merge(api_routes)
        .with_state(state)
}
