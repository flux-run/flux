//! Gateway request metrics middleware + Prometheus scrape endpoint.
//!
//! ## DB metrics (fire-and-forget)
//! After every response a non-blocking INSERT is sent to `gateway_metrics`
//! (status code + latency). A DB error never affects the response seen by
//! the client.
//!
//! ## Prometheus in-memory metrics (zero-cost on hot path)
//! The `record_metrics` middleware also emits:
//!   - `flux_requests_total{status}`  — counter incremented on every response
//!   - `flux_request_duration_ms`     — histogram of response latency in ms
//!
//! Initialise the recorder once at startup with `init_prometheus()`, then
//! expose `/internal/metrics` via `prometheus_handler()`.

use std::sync::OnceLock;
use std::time::Instant;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};
use uuid::Uuid;

use crate::state::SharedState;

/// Global handle to the Prometheus recorder — set once by `init_prometheus()`.
static PROM_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the Prometheus metrics recorder.  Must be called once at startup
/// before any `metrics::counter!` / `metrics::histogram!` calls are made.
/// Safe to call multiple times — only the first call takes effect.
pub fn init_prometheus() {
    if PROM_HANDLE.get().is_some() {
        return;
    }
    match PrometheusBuilder::new().install_recorder() {
        Ok(handle) => {
            let _ = PROM_HANDLE.set(handle);
        }
        Err(e) => {
            tracing::warn!("Failed to install Prometheus recorder: {}", e);
        }
    }
}

/// Axum handler: render the current Prometheus scrape output.
///
/// Route: `GET /internal/metrics`
pub async fn prometheus_handler() -> impl IntoResponse {
    match PROM_HANDLE.get() {
        Some(handle) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4; charset=utf-8")],
            handle.render(),
        )
            .into_response(),
        None => (StatusCode::SERVICE_UNAVAILABLE, "metrics not initialised").into_response(),
    }
}

pub async fn record_metrics(
    State(state): State<SharedState>,
    req: Request,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();
    let start = Instant::now();
    let response = next.run(req).await;

    let status = response.status().as_u16() as i32;
    let latency_ms = start.elapsed().as_millis() as i64;

    // ── Prometheus in-memory counters ──────────────────────────────────────
    let status_str = status.to_string();
    metrics::counter!("flux_requests_total", "status" => status_str, "path" => path)
        .increment(1);
    metrics::histogram!("flux_request_duration_ms").record(latency_ms as f64);

    // ── DB metrics (fire-and-forget) ───────────────────────────────────────
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
