use sqlx::PgPool;
use uuid::Uuid;

pub async fn lock_job(pool: &PgPool, job_id: Uuid) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE jobs SET status = 'running', locked_at = now(), updated_at = now() WHERE id = $1")
        .bind(job_id)
        .execute(pool)
        .await?;

    Ok(())
}