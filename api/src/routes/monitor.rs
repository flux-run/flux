use axum::extract::{Path, State};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    error::{ApiError, ApiResponse},
    AppState,
};

type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

fn db_err(e: sqlx::Error) -> ApiError {
    ApiError::internal(e.to_string())
}

pub async fn monitor_status(State(state): State<AppState>) -> ApiResult<Value> {
    // Verify DB is reachable.
    sqlx::query("SELECT 1")
        .execute(&state.pool)
        .await
        .map_err(db_err)?;

    // Gather live queue health numbers in a single query.
    let q = sqlx::query(
        "SELECT \
           COUNT(*) FILTER (WHERE status = 'pending')  AS pending, \
           COUNT(*) FILTER (WHERE status = 'running')  AS running, \
           COUNT(*) FILTER (WHERE status = 'failed')   AS failed \
         FROM jobs",
    )
    .fetch_one(&state.pool)
    .await
    .map_err(db_err)?;

    let pending: i64 = q.get("pending");
    let running: i64 = q.get("running");
    let failed:  i64 = q.get("failed");

    let dlq_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dead_letter_jobs")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    // Function deploy count.
    let fn_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM functions")
        .fetch_one(&state.pool)
        .await
        .unwrap_or(0);

    Ok(ApiResponse::new(json!({
        "status": "ok",
        "services": {
            "database": { "status": "ok" },
            "api":      { "status": "ok" },
        },
        "queue": {
            "pending": pending,
            "running": running,
            "failed":  failed,
            "dlq":     dlq_count,
        },
        "functions": fn_count,
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

// ── Alerts ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateAlertPayload {
    pub name:        String,
    pub metric:      String,
    pub threshold:   f64,
    #[serde(default = "default_condition")]
    pub condition:   String,
    #[serde(default = "default_window")]
    pub window_secs: i32,
}

fn default_condition() -> String { "above".into() }
fn default_window()    -> i32    { 300 }

pub async fn monitor_alerts_list(State(state): State<AppState>) -> ApiResult<Value> {
    #[derive(sqlx::FromRow)]
    struct AlertRow {
        id:           Uuid,
        name:         String,
        metric:       String,
        threshold:    f64,
        condition:    String,
        window_secs:  i32,
        enabled:      bool,
        created_at:   chrono::DateTime<chrono::Utc>,
        triggered_at: Option<chrono::DateTime<chrono::Utc>>,
        resolved_at:  Option<chrono::DateTime<chrono::Utc>>,
    }

    let rows = sqlx::query_as::<_, AlertRow>(
        "SELECT id, name, metric, threshold, condition, window_secs, enabled, \
                created_at, triggered_at, resolved_at \
         FROM flux.monitor_alerts \
         ORDER BY created_at DESC",
    )
    .fetch_all(&state.pool)
    .await
    .map_err(db_err)?;

    let alerts: Vec<Value> = rows.into_iter().map(|r| json!({
        "id":           r.id,
        "name":         r.name,
        "metric":       r.metric,
        "threshold":    r.threshold,
        "condition":    r.condition,
        "window_secs":  r.window_secs,
        "enabled":      r.enabled,
        "created_at":   r.created_at,
        "triggered_at": r.triggered_at,
        "resolved_at":  r.resolved_at,
    })).collect();

    let count = alerts.len();
    Ok(ApiResponse::new(json!({ "data": alerts, "count": count })))
}

pub async fn monitor_alert_create(
    State(state): State<AppState>,
    axum::Json(payload): axum::Json<CreateAlertPayload>,
) -> ApiResult<Value> {
    // Validate metric name to prevent injection through the stored value.
    let valid_metrics = [
        "error_rate", "latency_p95", "latency_p99",
        "queue_dlq", "queue_failed", "queue_pending",
    ];
    if !valid_metrics.contains(&payload.metric.as_str()) {
        return Err(ApiError::bad_request(format!(
            "metric must be one of: {}",
            valid_metrics.join(", ")
        )));
    }
    let valid_conditions = ["above", "below"];
    if !valid_conditions.contains(&payload.condition.as_str()) {
        return Err(ApiError::bad_request("condition must be 'above' or 'below'"));
    }
    if payload.window_secs < 60 || payload.window_secs > 86_400 {
        return Err(ApiError::bad_request("window_secs must be between 60 and 86400"));
    }

    let id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO flux.monitor_alerts \
             (id, name, metric, threshold, condition, window_secs) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(id)
    .bind(&payload.name)
    .bind(&payload.metric)
    .bind(payload.threshold)
    .bind(&payload.condition)
    .bind(payload.window_secs)
    .execute(&state.pool)
    .await
    .map_err(db_err)?;

    Ok(ApiResponse::new(json!({ "id": id, "created": true })))
}

pub async fn monitor_alert_delete(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Value> {
    sqlx::query("DELETE FROM flux.monitor_alerts WHERE id = $1")
        .bind(id)
        .execute(&state.pool)
        .await
        .map_err(db_err)?;

    Ok(ApiResponse::new(json!({ "deleted": true })))
}
