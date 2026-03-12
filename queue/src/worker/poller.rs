use std::sync::Arc;
use std::time::Duration;
use reqwest::Client;
use tokio::sync::Semaphore;
use tokio::time::sleep;
use sqlx::PgPool;
use tracing::error;
use crate::queue::fetch_jobs;
use crate::worker::executor;

pub async fn poll(pool: PgPool, runtime_url: String, service_token: String, concurrency: usize, poll_interval_ms: u64) {
    let client = Client::new();
    let semaphore = Arc::new(Semaphore::new(concurrency));

    loop {
        match fetch_jobs::fetch_and_lock_jobs(&pool, 20).await {
            Ok(jobs) => {
                for job in jobs {
                    let permit = semaphore.clone().acquire_owned().await.unwrap();
                    let pool_clone = pool.clone();
                    let runtime_url_clone = runtime_url.clone();
                    let service_token_clone = service_token.clone();
                    let client_clone = client.clone();

                    tokio::spawn(async move {
                        let _permit = permit; // dropped when task completes
                        executor::execute(pool_clone, runtime_url_clone, service_token_clone, client_clone, job).await;
                    });
                }
            }
            Err(e) => {
                error!("Failed to fetch jobs: {}", e);
            }
        }

        sleep(Duration::from_millis(poll_interval_ms)).await;
    }
}