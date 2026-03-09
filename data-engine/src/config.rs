use dotenvy::dotenv;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub struct Config {
    pub database_url: String,
    pub port: u16,
    /// Default row cap when the caller omits LIMIT.
    pub default_query_limit: i64,
    /// Hard ceiling — a client cannot exceed this even if they send limit=N.
    pub max_query_limit: i64,
    /// Base URL of the runtime service, used by the hooks engine.
    pub runtime_url: String,
    /// Maximum query complexity score. Requests exceeding this are rejected
    /// with HTTP 400 before any database work is performed.
    /// Default: 1000. Set to 0 to disable the check.
    pub max_query_complexity: u64,
    /// Query execution timeout in milliseconds.
    /// The timer starts after compilation and covers the full execute phase.
    /// Default: 30 000 ms (30 s).
    pub query_timeout_ms: u64,
    /// Maximum relationship nesting depth in a single query.
    /// depth=1 is a single join, depth=4+ triggers batched execution.
    /// Requests deeper than this ceiling are rejected with HTTP 400.
    /// Default: 6. Set to 0 to disable.
    pub max_nest_depth: usize,
    /// S3 bucket name for file storage. None = file engine disabled.
    pub s3_bucket: Option<String>,
    /// AWS region (default: us-east-1).
    pub s3_region: String,
    /// Optional custom endpoint for MinIO / Localstack.
    pub s3_endpoint: Option<String>,
}

pub fn load() -> Config {
    Config {
        database_url: std::env::var("DATABASE_URL").expect("DATABASE_URL required"),
        port: std::env::var("PORT")
            .or_else(|_| std::env::var("DATA_ENGINE_PORT"))
            .unwrap_or_else(|_| "8080".to_string())
            .parse()
            .expect("PORT must be a number"),
        default_query_limit: std::env::var("DEFAULT_QUERY_LIMIT")
            .unwrap_or_else(|_| "100".to_string())
            .parse()
            .expect("DEFAULT_QUERY_LIMIT must be a positive integer"),
        max_query_limit: std::env::var("MAX_QUERY_LIMIT")
            .unwrap_or_else(|_| "5000".to_string())
            .parse()
            .expect("MAX_QUERY_LIMIT must be a positive integer"),
        runtime_url: std::env::var("RUNTIME_URL")
            .unwrap_or_else(|_| "http://localhost:8082".to_string()),
        max_query_complexity: std::env::var("MAX_QUERY_COMPLEXITY")
            .unwrap_or_else(|_| "1000".to_string())
            .parse()
            .expect("MAX_QUERY_COMPLEXITY must be a non-negative integer"),
        query_timeout_ms: std::env::var("QUERY_TIMEOUT_MS")
            .unwrap_or_else(|_| "30000".to_string())
            .parse()
            .expect("QUERY_TIMEOUT_MS must be a non-negative integer"),
        max_nest_depth: std::env::var("MAX_NEST_DEPTH")
            .unwrap_or_else(|_| "6".to_string())
            .parse()
            .expect("MAX_NEST_DEPTH must be a non-negative integer"),
        s3_bucket: std::env::var("S3_BUCKET").ok(),
        s3_region: std::env::var("S3_REGION")
            .unwrap_or_else(|_| "us-east-1".to_string()),
        s3_endpoint: std::env::var("S3_ENDPOINT").ok(),
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
