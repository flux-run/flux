use reqwest::Client;
use sqlx::PgPool;
use crate::models::job::Job;
use crate::services::retry_service;
use crate::queue::update_status::update_status;

pub async fn execute(pool: PgPool, runtime_url: String, client: Client, job: Job) {
    let runtime_endpoint = format!("{}/internal/execute", runtime_url.trim_end_matches('/'));

    let res = client.post(runtime_endpoint)
        .json(&serde_json::json!({"function_id": job.function_id, "payload": job.payload}))
        .send().await;

    if let Ok(response) = res {
        if response.status().is_success() {
            let _ = update_status(&pool, job.id, "completed").await;
        } else {
            handle_failure(&pool, job).await;
        }
    } else {
        handle_failure(&pool, job).await;
    }
}

async fn handle_failure(pool: &PgPool, job: Job) {
    let new_attempts = job.attempts + 1;
    if new_attempts < job.max_attempts {
        let _ = retry_service::retry_job(pool, job.id, new_attempts).await;
    } else {
        let _ = retry_service::dead_letter_job(pool, job.id, "execution failed").await;
    }
}