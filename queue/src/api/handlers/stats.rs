use std::sync::Arc;
use axum::{extract::State, http::StatusCode, Json};
use sqlx::FromRow;
use crate::state::AppState;

#[derive(FromRow)]
struct JobCounts {
    pending: i64,
    running: i64,
    completed: i64,
    failed: i64,
    cancelled: i64,
}

#[derive(FromRow)]
struct LatencyStats {
    avg_queue_time_ms: Option<i64>,
    p95_queue_time_ms: Option<i64>,
    avg_execution_time_ms: Option<i64>,
    p95_execution_time_ms: Option<i64>,
}

#[derive(FromRow)]
struct RetryStats {
    total_retries: Option<i64>,
    jobs_retried: Option<i64>,
    max_retries_seen: Option<i32>,
}

pub async fn handler(State(state): State<Arc<AppState>>) -> (StatusCode, Json<serde_json::Value>) {
    let pool = &state.pool;

    // 1. Job status counts
    let counts = sqlx::query_as::<_, JobCounts>(
        "SELECT
            COUNT(*) FILTER (WHERE status = 'pending')   AS pending,
            COUNT(*) FILTER (WHERE status = 'running')   AS running,
            COUNT(*) FILTER (WHERE status = 'completed') AS completed,
            COUNT(*) FILTER (WHERE status = 'failed')    AS failed,
            COUNT(*) FILTER (WHERE status = 'cancelled') AS cancelled
         FROM jobs",
    )
    .fetch_one(pool)
    .await;

    // 2. Dead-letter count
    let dead_letter_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM dead_letter_jobs")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    // 3. Latency percentiles (completed jobs only where started_at is set)
    let latency = sqlx::query_as::<_, LatencyStats>(
        "SELECT
            ROUND(AVG(
                EXTRACT(EPOCH FROM (started_at - created_at)) * 1000
            ))::bigint AS avg_queue_time_ms,

            ROUND(percentile_cont(0.95) WITHIN GROUP (
                ORDER BY EXTRACT(EPOCH FROM (started_at - created_at)) * 1000
            ))::bigint AS p95_queue_time_ms,

            ROUND(AVG(
                EXTRACT(EPOCH FROM (updated_at - started_at)) * 1000
            ))::bigint AS avg_execution_time_ms,

            ROUND(percentile_cont(0.95) WITHIN GROUP (
                ORDER BY EXTRACT(EPOCH FROM (updated_at - started_at)) * 1000
            ))::bigint AS p95_execution_time_ms

         FROM jobs
         WHERE status = 'completed'
           AND started_at IS NOT NULL",
    )
    .fetch_one(pool)
    .await;

    // 4. Retry statistics
    let retries = sqlx::query_as::<_, RetryStats>(
        "SELECT
            SUM(attempts)::bigint                           AS total_retries,
            COUNT(*) FILTER (WHERE attempts > 0)::bigint   AS jobs_retried,
            MAX(attempts)                                   AS max_retries_seen
         FROM jobs",
    )
    .fetch_one(pool)
    .await;

    match counts {
        Ok(c) => {
            let lat = latency.ok();
            let ret = retries.ok();
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "queue": {
                        "pending":     c.pending,
                        "running":     c.running,
                        "completed":   c.completed,
                        "failed":      c.failed,
                        "cancelled":   c.cancelled,
                        "dead_letter": dead_letter_count,
                    },
                    "latency_ms": {
                        "avg_queue_time":      lat.as_ref().and_then(|l| l.avg_queue_time_ms),
                        "p95_queue_time":      lat.as_ref().and_then(|l| l.p95_queue_time_ms),
                        "avg_execution_time":  lat.as_ref().and_then(|l| l.avg_execution_time_ms),
                        "p95_execution_time":  lat.as_ref().and_then(|l| l.p95_execution_time_ms),
                    },
                    "retries": {
                        "total":       ret.as_ref().and_then(|r| r.total_retries),
                        "jobs_retried":ret.as_ref().and_then(|r| r.jobs_retried),
                        "max_seen":    ret.as_ref().and_then(|r| r.max_retries_seen),
                    }
                }))
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error":   "FUNCTION_ERROR",
                "message": e.to_string(),
                "code":    500,
            })),
        ),
    }
}
