//! Retry service — decides what happens when a job execution fails.
//!
//! ## Retry logic
//!
//! When the runtime returns an error (non-2xx) or the HTTP call itself fails:
//! - If `attempts < max_attempts`: the job is reset to `pending` with a `run_at` in the
//!   future, computed by [`crate::worker::backoff::retry_delay`] (5 s × 2^attempts).
//! - If `attempts >= max_attempts`: the job is moved to `dead_letter_jobs` with an error
//!   reason string, and the original row is deleted from `flux.jobs`.
//!
//! ## Dead-letter conditions
//!
//! A job ends up in `dead_letter_jobs` when any of the following occur:
//! 1. It has been retried `max_attempts` times and still fails.
//! 2. It times out (`started_at + max_runtime_seconds < now()`) and has exhausted retries
//!    (handled by [`crate::worker::timeout_recovery`]).
//!
//! Dead-lettered jobs are never automatically retried; they require manual intervention
//! via `POST /jobs/{id}/retry` (which re-enqueues the job with `attempts = 0`).
use sqlx::PgPool;
use crate::worker::backoff;
use uuid::Uuid;

pub async fn retry_job(pool: &PgPool, job_id: Uuid, attempts: i32) -> Result<(), sqlx::Error> {
    let delay = backoff::retry_delay(attempts as u32);
    sqlx::query(
        "UPDATE jobs \
         SET status = 'pending', attempts = $1, run_at = now() + ($2 * interval '1 second'), updated_at = now() \
         WHERE id = $3",
    )
    .bind(attempts)
    .bind(delay.as_secs() as i64)
    .bind(job_id)
    .execute(pool)
    .await?;

    Ok(())
}

pub async fn dead_letter_job(pool: &PgPool, job_id: Uuid, error: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO dead_letter_jobs (id, tenant_id, project_id, function_id, payload, error, failed_at) \
         SELECT id, tenant_id, project_id, function_id, payload, $1, now() FROM jobs WHERE id = $2",
    )
    .bind(error)
    .bind(job_id)
    .execute(pool)
    .await?;

    sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(job_id)
        .execute(pool)
        .await?;

    Ok(())
}