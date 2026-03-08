use sqlx::PgPool;
use crate::models::job::Job;

pub async fn fetch_and_lock_jobs(pool: &PgPool, batch_size: i64) -> Result<Vec<Job>, sqlx::Error> {
    sqlx::query_as::<_, Job>(
        "UPDATE jobs j
         SET status = 'running', locked_at = now(), updated_at = now()
         WHERE j.id IN (
             SELECT id
             FROM jobs
             WHERE status = 'pending' AND run_at <= now()
             ORDER BY run_at
             LIMIT $1
             FOR UPDATE SKIP LOCKED
         )
         RETURNING j.*",
    )
    .bind(batch_size)
    .fetch_all(pool)
    .await
}