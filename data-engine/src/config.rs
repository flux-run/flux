use dotenvy::dotenv;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub struct Config {
    pub database_url: String,
    pub port: u16,
    /// Hard cap on rows returned from SELECT when the caller omits a LIMIT.
    pub default_query_limit: i64,
}

pub fn load() -> Config {
    Config {
        database_url: std::env::var("DATABASE_URL").expect("DATABASE_URL required"),
        port: std::env::var("PORT")
            .or_else(|_| std::env::var("DATA_ENGINE_PORT"))
            .unwrap_or_else(|_| "8084".to_string())
            .parse()
            .expect("PORT must be a number"),
        default_query_limit: std::env::var("DEFAULT_QUERY_LIMIT")
            .unwrap_or_else(|_| "1000".to_string())
            .parse()
            .expect("DEFAULT_QUERY_LIMIT must be a positive integer"),
    }
}

pub fn init() {
    dotenv().ok();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "data_engine=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}
