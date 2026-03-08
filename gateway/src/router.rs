use axum::{
    routing::{any},
    Router,
};
use crate::state::SharedState;
use crate::routes::proxy::proxy_handler;

pub fn create_router(state: SharedState) -> Router {
    Router::new()
        .route("/{*path}", any(proxy_handler))
        .with_state(state)
}
