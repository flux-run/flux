use reqwest::Client;
use sqlx::PgPool;
use tracing::{info, warn, error};
use uuid::Uuid;
use crate::models::job::Job;
use crate::services::retry_service;
use crate::queue::update_status::update_status;

async fn log_job(pool: &PgPool, job_id: Uuid, message: &str) {
    let _ = sqlx::query(
        "INSERT INTO job_logs (id, job_id, message) VALUES ($1, $2, $3)",
    )
    .bind(Uuid::new_v4())
    .bind(job_id)
    .bind(message)
    .execute(pool)
    .await;
}

pub async fn execute(pool: PgPool, runtime_url: String, service_token: String, client: Client, job: Job) {
    info!(job_id = %job.id, function_id = %job.function_id, "job started");
    log_job(&pool, job.id, "job started").await;

    // Generate a fresh request_id for this execution. This UUID is forwarded to
    // the runtime as `x-request-id` so all spans emitted during the execution are
    // grouped under it — enabling `flux trace <request_id>` on a job.
    let request_id = Uuid::new_v4();

    // Stamp started_at and request_id before the runtime call.
    // Timeout recovery measures elapsed time from started_at, not from locked_at.
    let _ = sqlx::query(
        "UPDATE jobs SET started_at = now(), request_id = $1, updated_at = now() WHERE id = $2",
    )
    .bind(request_id)
    .bind(job.id)
    .execute(&pool)
    .await;

    let runtime_endpoint = format!("{}/execute", runtime_url.trim_end_matches('/'));

    let res = client
        .post(&runtime_endpoint)
        .bearer_auth(&service_token)
        .header("x-request-id", request_id.to_string())
        .json(&serde_json::json!({
            "function_id": job.function_id,
            "project_id":  job.project_id,
            "payload":     job.payload
        }))
        .send()
        .await;

    match res {
        Ok(response) if response.status().is_success() => {
            let _ = update_status(&pool, job.id, "completed").await;
            info!(job_id = %job.id, %request_id, "job completed");
            log_job(&pool, job.id, &format!("job completed (trace: {})", request_id)).await;
        }
        Ok(response) => {
            let status = response.status();
            error!(job_id = %job.id, %request_id, %status, "runtime returned error");
            log_job(&pool, job.id, &format!("job failed: runtime returned {}", status)).await;
            handle_failure(&pool, job).await;
        }
        Err(e) => {
            error!(job_id = %job.id, %request_id, error = %e, "runtime request failed");
            log_job(&pool, job.id, &format!("job failed: {}", e)).await;
            handle_failure(&pool, job).await;
        }
    }
}

async fn handle_failure(pool: &PgPool, job: Job) {
    let new_attempts = job.attempts + 1;
    if new_attempts < job.max_attempts {
        warn!(job_id = %job.id, attempts = new_attempts, "retry scheduled");
        log_job(pool, job.id, &format!("retry scheduled (attempt {})", new_attempts)).await;
        let _ = retry_service::retry_job(pool, job.id, new_attempts).await;
    } else {
        error!(job_id = %job.id, "retry limit reached, moving to dead letter");
        log_job(pool, job.id, "retry limit reached, moved to dead letter").await;
        let _ = retry_service::dead_letter_job(pool, job.id, "execution failed after max attempts").await;
    }
}
