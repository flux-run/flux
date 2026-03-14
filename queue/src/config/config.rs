use dotenvy::dotenv;
use serde::Deserialize;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Deserialize)]
pub struct Config {
    pub database_url: String,
    pub api_url: String,
    pub runtime_url: String,
    pub service_token: String,
    pub port: u16,
    pub worker_concurrency: usize,
    pub poll_interval_ms: u64,
    pub job_timeout_check_interval_ms: u64,
}

pub fn load() -> Config {
    Config {
        database_url: std::env::var("DATABASE_URL").expect("DATABASE_URL required"),
        api_url: std::env::var("API_URL")
            .unwrap_or_else(|_| "http://localhost:8080/flux/api".to_string()),
        runtime_url: std::env::var("RUNTIME_URL")
            .unwrap_or_else(|_| "http://localhost:8083".to_string()),
        service_token: std::env::var("INTERNAL_SERVICE_TOKEN")
            .or_else(|_| std::env::var("SERVICE_TOKEN"))
            .unwrap_or_else(|_| {
                if std::env::var("FLUX_ENV").as_deref() == Ok("production") {
                    panic!(
                        "[Flux] INTERNAL_SERVICE_TOKEN must be set in production. \
                         The queue service cannot start without it."
                    );
                }
                tracing::warn!(
                    "[Flux] INTERNAL_SERVICE_TOKEN not set — using insecure default 'stub_token'. \
                     Set INTERNAL_SERVICE_TOKEN in production."
                );
                "stub_token".to_string()
            }),
        port: std::env::var("PORT")
            .or_else(|_| std::env::var("QUEUE_PORT"))
            .unwrap_or_else(|_| "8084".to_string())
            .parse()
            .expect("PORT must be a number"),
        worker_concurrency: std::env::var("WORKER_CONCURRENCY")
            .unwrap_or_else(|_| "50".to_string())
            .parse()
            .expect("WORKER_CONCURRENCY must be a number"),
        poll_interval_ms: std::env::var("WORKER_POLL_INTERVAL_MS")
            .unwrap_or_else(|_| "200".to_string())
            .parse()
            .expect("WORKER_POLL_INTERVAL_MS must be a number"),
        job_timeout_check_interval_ms: std::env::var("JOB_TIMEOUT_CHECK_INTERVAL_MS")
            .unwrap_or_else(|_| "30000".to_string())
            .parse()
            .expect("JOB_TIMEOUT_CHECK_INTERVAL_MS must be a number"),
    }
}

pub fn init() {
    dotenv().ok();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "queue=debug,tower_http=debug,axum::rejection=trace".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .try_init().ok();
}