use std::time::Duration;
use reqwest::Client;
use tokio::time::sleep;
use sqlx::PgPool;
use crate::queue::fetch_jobs;
use crate::worker::executor;

pub async fn poll(pool: PgPool, runtime_url: String) {
    let client = Client::new();

    loop {
        let jobs = fetch_jobs::fetch_and_lock_jobs(&pool, 20).await.unwrap_or_default();

        for job in jobs {
            let pool_clone = pool.clone();
            let runtime_url_clone = runtime_url.clone();
            let client_clone = client.clone();

            tokio::spawn(async move {
                executor::execute(pool_clone, runtime_url_clone, client_clone, job).await;
            });
        }

        sleep(Duration::from_millis(200)).await;
    }
}