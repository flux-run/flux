use sqlx::PgPool;
use crate::models::job::Job;
use uuid::Uuid;
use chrono::NaiveDateTime;

pub struct CreateJobInput {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub function_id: Uuid,
    pub payload: serde_json::Value,
    pub run_at: NaiveDateTime,
    pub max_attempts: i32,
    pub idempotency_key: Option<String>,
}

pub async fn create_job(pool: &PgPool, input: CreateJobInput) -> Result<Uuid, sqlx::Error> {
    let record = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO jobs (tenant_id, project_id, function_id, payload, run_at, max_attempts, idempotency_key) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) \
         ON CONFLICT (idempotency_key) WHERE idempotency_key IS NOT NULL \
         DO UPDATE SET updated_at = jobs.updated_at \
         RETURNING id",
    )
    .bind(input.tenant_id)
    .bind(input.project_id)
    .bind(input.function_id)
    .bind(input.payload)
    .bind(input.run_at)
    .bind(input.max_attempts)
    .bind(input.idempotency_key)
    .fetch_one(pool)
    .await?;

    Ok(record)
}

pub async fn get_job(pool: &PgPool, id: Uuid) -> Result<Job, sqlx::Error> {
    sqlx::query_as::<_, Job>("SELECT * FROM jobs WHERE id = $1")
        .bind(id)
        .fetch_one(pool)
        .await
}

/// List jobs with optional status filter.
///
/// Used by `flux queue list`: returns jobs ordered by `run_at` descending
/// (most-recently-scheduled first) with simple limit/offset pagination.
pub async fn list_jobs(
    pool: &PgPool,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> Result<Vec<Job>, sqlx::Error> {
    match status {
        Some(s) => {
            sqlx::query_as::<_, Job>(
                "SELECT * FROM jobs WHERE status = $1 ORDER BY run_at DESC LIMIT $2 OFFSET $3",
            )
            .bind(s)
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
        None => {
            sqlx::query_as::<_, Job>(
                "SELECT * FROM jobs ORDER BY run_at DESC LIMIT $1 OFFSET $2",
            )
            .bind(limit)
            .bind(offset)
            .fetch_all(pool)
            .await
        }
    }
}