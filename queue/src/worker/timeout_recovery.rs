use std::time::Duration;
use sqlx::{FromRow, PgPool};
use tokio::time::sleep;
use tracing::{info, warn, error};
use uuid::Uuid;

#[derive(FromRow)]
struct StuckJob {
    id: Uuid,
    attempts: i32,
    max_attempts: i32,
}

/// Background loop that rescues jobs stuck in `running` state.
///
/// A job is considered stuck when:
///   `locked_at < now() - (max_runtime_seconds * interval '1 second')`
///
/// Each timed-out job has its attempt count incremented:
/// - If `attempts < max_attempts`: reset to `pending` so a worker picks it up again.
/// - If `attempts >= max_attempts`: moved to `dead_letter_jobs`.
pub async fn run(pool: PgPool, check_interval_ms: u64) {
    loop {
        sleep(Duration::from_millis(check_interval_ms)).await;

        match recover_stuck_jobs(&pool).await {
            Ok(0) => {}
            Ok(n) => info!(count = n, "timeout recovery: reset {} stuck job(s)", n),
            Err(e) => error!("timeout recovery error: {}", e),
        }
    }
}

async fn recover_stuck_jobs(pool: &PgPool) -> Result<u64, sqlx::Error> {
    // Atomically claim all stuck running jobs beyond their max_runtime_seconds.
    let rows = sqlx::query_as::<_, StuckJob>(
        "UPDATE jobs
         SET attempts = attempts + 1, locked_at = NULL, updated_at = now()
         WHERE status = 'running'
           AND locked_at IS NOT NULL
           AND locked_at < now() - (max_runtime_seconds * interval '1 second')
         RETURNING id, attempts, max_attempts",
    )
    .fetch_all(pool)
    .await?;

    let count = rows.len() as u64;

    for stuck in rows {
        if stuck.attempts >= stuck.max_attempts {
            dead_letter_timed_out(pool, stuck).await;
        } else {
            reset_to_pending(pool, stuck).await;
        }
    }

    Ok(count)
}

async fn reset_to_pending(pool: &PgPool, job: StuckJob) {
    warn!(
        job_id = %job.id,
        attempts = job.attempts,
        "job timed out — resetting to pending for retry"
    );
    let _ = sqlx::query(
        "UPDATE jobs SET status = 'pending', updated_at = now() WHERE id = $1",
    )
    .bind(job.id)
    .execute(pool)
    .await;

    let _ = sqlx::query(
        "INSERT INTO job_logs (id, job_id, message) VALUES ($1, $2, $3)",
    )
    .bind(Uuid::new_v4())
    .bind(job.id)
    .bind(format!("timed out — reset to pending (attempt {})", job.attempts))
    .execute(pool)
    .await;
}

async fn dead_letter_timed_out(pool: &PgPool, job: StuckJob) {
    error!(
        job_id = %job.id,
        attempts = job.attempts,
        "job timed out and exhausted retries — moving to dead letter"
    );
    let _ = sqlx::query(
        "INSERT INTO dead_letter_jobs (id, tenant_id, project_id, function_id, payload, error, failed_at)
         SELECT id, tenant_id, project_id, function_id, payload, $1, now()
         FROM jobs WHERE id = $2",
    )
    .bind("timed out after max attempts")
    .bind(job.id)
    .execute(pool)
    .await;

    let _ = sqlx::query("DELETE FROM jobs WHERE id = $1")
        .bind(job.id)
        .execute(pool)
        .await;
}
