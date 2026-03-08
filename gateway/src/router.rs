use axum::{
    routing::{any},
    Router,
};
use crate::state::SharedState;
use crate::routes::proxy::proxy_handler;

pub fn create_router(state: SharedState) -> Router {
    Router::new()
        .route("/{*path}", any(proxy_handler))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            crate::middleware::identity_resolver::resolve_identity,
        ))
        .with_state(state)
}
