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