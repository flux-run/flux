use sqlx::PgPool;
use crate::models::job::Job;
use uuid::Uuid;
use chrono::NaiveDateTime;

pub struct CreateJobInput {
    pub tenant_id: Uuid,
    pub project_id: Uuid,
    pub job_type: String,
    pub function_id: Option<Uuid>,
    pub payload: serde_json::Value,
    pub run_at: NaiveDateTime,
    pub max_attempts: i32,
}

pub async fn create_job(pool: &PgPool, input: CreateJobInput) -> Result<Uuid, sqlx::Error> {
    let record = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO jobs (tenant_id, project_id, type, function_id, payload, run_at, max_attempts) \
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
    )
    .bind(input.tenant_id)
    .bind(input.project_id)
    .bind(input.job_type)
    .bind(input.function_id)
    .bind(input.payload)
    .bind(input.run_at)
    .bind(input.max_attempts)
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