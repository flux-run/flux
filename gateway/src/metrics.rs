//! Gateway request metrics middleware.
//!
//! After every response we fire-and-forget an INSERT into `gateway_metrics`
//! (status code + latency). All writes are non-blocking — a DB error never
//! affects the response seen by the client.

use std::time::Instant;

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::Response,
};
use uuid::Uuid;

use crate::state::SharedState;

pub async fn record_metrics(
    State(state): State<SharedState>,
    req: Request,
    next: Next,
) -> Response {
    let start = Instant::now();
    let response = next.run(req).await;

    let status = response.status().as_u16() as i32;
    let latency_ms = start.elapsed().as_millis() as i64;
    let pool = state.db_pool.clone();

    tokio::spawn(async move {
        let _ = sqlx::query(
            "INSERT INTO gateway_metrics (id, status, latency_ms, created_at) \
             VALUES ($1, $2, $3, now())",
        )
        .bind(Uuid::new_v4())
        .bind(status)
        .bind(latency_ms)
        .execute(&pool)
        .await;
    });

    response
}
