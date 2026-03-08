use dotenvy::dotenv;
use serde::Deserialize;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Deserialize)]
pub struct Config {
    pub database_url: String,
    pub runtime_url: String,
    pub port: u16,
    pub worker_concurrency: usize,
    pub poll_interval_ms: u64,
}

pub fn load() -> Config {
    Config {
        database_url: std::env::var("DATABASE_URL").expect("DATABASE_URL required"),
        runtime_url: std::env::var("RUNTIME_URL")
            .unwrap_or_else(|_| "http://localhost:3002".to_string()),
        port: std::env::var("PORT")
            .or_else(|_| std::env::var("QUEUE_PORT"))
            .unwrap_or_else(|_| "8083".to_string())
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
        .init();
}