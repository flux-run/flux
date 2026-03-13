use axum::extract::State;
use serde_json::{json, Value};
use sqlx::Row;

use crate::{
    error::{ApiError, ApiResponse},
    AppState,
};

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err(e: sqlx::Error) -> ApiError {
    ApiError::internal(e.to_string())
}

pub async fn monitor_status(State(state): State<AppState>) -> ApiResult<Value> {
    sqlx::query("SELECT 1")
        .execute(&state.pool)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(json!({
        "status": "ok",
        "services": {
            "database": { "status": "ok" },
            "api":      { "status": "ok" },
        }
    })))
}

pub async fn monitor_metrics(State(state): State<AppState>) -> ApiResult<Value> {
    let row = sqlx::query(
        "SELECT \
           COUNT(*) as total, \
           COUNT(*) FILTER (WHERE status >= 500) as errors, \
           COALESCE(PERCENTILE_CONT(0.5) WITHIN GROUP (ORDER BY latency_ms), 0) as p50, \
           COALESCE(PERCENTILE_CONT(0.95) WITHIN GROUP (ORDER BY latency_ms), 0) as p95, \
           COALESCE(PERCENTILE_CONT(0.99) WITHIN GROUP (ORDER BY latency_ms), 0) as p99 \
         FROM flux.gateway_metrics \
         WHERE created_at > now() - interval '1 hour'",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

    let total: i64 = row.get("total");
    let errors: i64 = row.get("errors");
    let p50: f64 = row.get("p50");
    let p95: f64 = row.get("p95");
    let p99: f64 = row.get("p99");

    Ok(ApiResponse::new(json!({
        "data": {
            "requests_total": total,
            "errors_total":   errors,
            "p50_ms":         p50,
            "p95_ms":         p95,
            "p99_ms":         p99,
        },
        "window": "1h"
    })))
}
