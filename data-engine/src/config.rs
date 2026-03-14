use dotenvy::dotenv;

pub struct Config {
    pub database_url: String,
    pub port: u16,
    /// Default row cap when the caller omits LIMIT.
    pub default_query_limit: i64,
    /// Hard ceiling — a client cannot exceed this even if they send limit=N.
    pub max_query_limit: i64,
    /// Base URL of the runtime service, used by the cron worker.
    pub runtime_url: String,
    /// Maximum query complexity score. Requests exceeding this are rejected
    /// with HTTP 400 before any database work is performed.
    pub max_query_complexity: u64,
    /// Query execution timeout in milliseconds.
    pub query_timeout_ms: u64,
    /// Postgres-level statement timeout (ms) injected as `SET LOCAL statement_timeout`.
    /// Replay and internal operations use 6× this value.
    pub statement_timeout_ms: u64,
    /// Maximum relationship nesting depth in a single query.
    pub max_nest_depth: usize,
    // ── Retention ────────────────────────────────────────────────────────────
    pub record_retention_days: u32,
    pub error_retention_days: u32,
    pub retention_job_hour: u32,
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
        statement_timeout_ms: std::env::var("STATEMENT_TIMEOUT_MS")
            .unwrap_or_else(|_| "5000".to_string())
            .parse()
            .expect("STATEMENT_TIMEOUT_MS must be a non-negative integer"),
        max_nest_depth: std::env::var("MAX_NEST_DEPTH")
            .unwrap_or_else(|_| "6".to_string())
            .parse()
            .expect("MAX_NEST_DEPTH must be a non-negative integer"),
        record_retention_days: std::env::var("RECORD_RETENTION_DAYS")
            .unwrap_or_else(|_| "30".to_string())
            .parse()
            .expect("RECORD_RETENTION_DAYS must be a positive integer"),
        error_retention_days: std::env::var("ERROR_RETENTION_DAYS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        retention_job_hour: std::env::var("RETENTION_JOB_HOUR")
            .unwrap_or_else(|_| "3".to_string())
            .parse()
            .expect("RETENTION_JOB_HOUR must be 0–23"),
    }
}

/// Load `.env` file if present. Must be called before `load()`.
pub fn init() {
    dotenv().ok();
}
