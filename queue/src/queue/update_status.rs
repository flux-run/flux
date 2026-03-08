use sqlx::PgPool;
use uuid::Uuid;

pub async fn update_status(pool: &PgPool, job_id: Uuid, status: &str) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE jobs SET status = $1, updated_at = now() WHERE id = $2")
        .bind(status)
        .bind(job_id)
        .execute(pool)
        .await?;

    Ok(())
}