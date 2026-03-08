use dotenvy::dotenv;
use serde::Deserialize;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Deserialize)]
pub struct Config {
    pub database_url: String,
    pub runtime_url: String,
    pub port: u16,
}

pub fn load() -> Config {
    Config {
        database_url: std::env::var("DATABASE_URL").expect("DATABASE_URL required"),
        runtime_url: std::env::var("RUNTIME_URL")
            .unwrap_or_else(|_| "http://localhost:3002".to_string()),
        port: std::env::var("PORT")
            .or_else(|_| std::env::var("QUEUE_PORT"))
            .unwrap_or_else(|_| "8082".to_string())
            .parse()
            .expect("PORT must be a number"),
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