//! Job execution — fetches a locked job, dispatches to the runtime, records outcome.
//!
//! ## Single responsibility
//!
//! This module orchestrates one job execution. Each sub-concern is delegated:
//! - **Log I/O** → [`super::span_emitter::QueueSpanEmitter`] (SRP)
//! - **DB status updates** → [`crate::queue::update_status`] (SRP)
//! - **Retry / dead-letter logic** → [`crate::services::retry_service`] (SRP)
//! - **Runtime dispatch** → reqwest HTTP client (DIP — will become a `RuntimeDispatch`
//!   trait call when the queue moves fully in-process)
//!
//! ## request_id chain
//!
//! A fresh UUIDv4 `request_id` is generated at the start of every execution.
//! It is:
//! - stamped on `flux.jobs.request_id` so `flux trace <request_id>` can find the job
//! - forwarded to the runtime as `x-request-id` so all spans emitted during the
//!   execution (runtime, data-engine, hooks) share the same request context
//! - attached to every `QueueSpanEmitter` span so queue + runtime spans appear
//!   together in the trace timeline

use std::sync::Arc;
use sqlx::PgPool;
use tracing::{info, warn, error};
use uuid::Uuid;
use job_contract::dispatch::{ApiDispatch, ExecuteRequest, RuntimeDispatch};
use crate::models::job::Job;
use crate::services::retry_service;
use crate::queue::update_status::update_status;
use super::span_emitter::QueueSpanEmitter;

/// Execute one job end-to-end.
///
/// Called from the poller; runs inside a `tokio::spawn`. Errors are handled
/// internally — this function never propagates failures to the caller.
pub async fn execute(
    pool:          PgPool,
    api:           Arc<dyn ApiDispatch>,
    runtime:       Arc<dyn RuntimeDispatch>,
    _service_token: String,
    job:           Job,
) {
    info!(job_id = %job.id, function_id = %job.function_id, "job started");

    // Each job execution gets a fresh request_id so all spans emitted by the
    // runtime during this job are grouped under it — enabling `flux trace <id>`.
    let request_id = Uuid::new_v4();

    // Stamp started_at and request_id so timeout_recovery can measure elapsed time
    // from a stable baseline, and so flux trace can find the execution record.
    let _ = sqlx::query(
        "UPDATE flux.jobs SET started_at = now(), request_id = $1, updated_at = now() WHERE id = $2",
    )
    .bind(request_id)
    .bind(job.id)
    .execute(&pool)
    .await;

    let emitter = QueueSpanEmitter::new(
        api,
        job.id,
        job.function_id,
        request_id.to_string(),
    );

    emitter.emit("info", format!("job started (attempt {})", job.attempts + 1), "start");

    let req = ExecuteRequest {
        function_id:    job.function_id.to_string(),
        payload:        job.payload.clone(),
        execution_seed: None,
        request_id:     Some(request_id.to_string()),
        parent_span_id: None,
        runtime_hint:   None,
        user_id:        None,
        jwt_claims:     None,
    };

    match runtime.execute(req).await {
        Ok(resp) if resp.status < 400 => {
            let _ = update_status(&pool, job.id, "completed").await;
            info!(job_id = %job.id, %request_id, "job completed");
            emitter.emit("info", format!("job completed (trace: {})", request_id), "end");
        }
        Ok(resp) => {
            error!(job_id = %job.id, %request_id, status = resp.status, "runtime returned error");
            emitter.emit("error", format!("job failed: runtime returned {}", resp.status), "error");
            handle_failure(&pool, &emitter, job).await;
        }
        Err(e) => {
            error!(job_id = %job.id, %request_id, error = %e, "runtime dispatch failed");
            emitter.emit("error", format!("job failed: {}", e), "error");
            handle_failure(&pool, &emitter, job).await;
        }
    }
}

/// Handle a job execution failure: schedule a retry or move to dead letter.
async fn handle_failure(pool: &PgPool, emitter: &QueueSpanEmitter, job: Job) {
    let new_attempts = job.attempts + 1;
    if new_attempts < job.max_attempts {
        warn!(job_id = %job.id, attempts = new_attempts, "retry scheduled");
        emitter.emit("warn", format!("retry scheduled (attempt {})", new_attempts), "event");
        let _ = retry_service::retry_job(pool, job.id, new_attempts).await;
    } else {
        error!(job_id = %job.id, "retry limit reached, moving to dead letter");
        emitter.emit("error", "retry limit reached, moved to dead letter".into(), "event");
        let _ = retry_service::dead_letter_job(pool, job.id, "execution failed after max attempts").await;
    }
}
