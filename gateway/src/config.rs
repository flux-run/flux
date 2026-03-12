use std::env;

/// Gateway runtime configuration — all values sourced from environment
/// variables.  See `.env.example` for a full listing.
///
/// Env vars mirror the table in `docs/gateway.md`:
///
/// | Env var                  | Default                 | Required |
/// |--------------------------|-------------------------|----------|
/// | `DATABASE_URL`           | —                       | yes      |
/// | `INTERNAL_SERVICE_TOKEN` | —                       | yes      |
/// | `PORT`                   | `8081`                  |          |
/// | `RUNTIME_URL`            | `http://localhost:8083` |          |
/// | `CONTROL_PLANE_URL`      | `http://localhost:8080` |          |
/// | `MAX_REQUEST_SIZE_BYTES` | `10485760` (10 MB)      |          |
/// | `RUNTIME_TIMEOUT_SECS`   | `30`                    |          |
/// | `RATE_LIMIT_PER_SEC`     | `50`                    |          |
/// | `LOCAL_MODE`             | `false`                 |          |
#[derive(Clone)]
pub struct Config {
    /// Postgres connection URL.
    pub database_url: String,
    /// Shared service-to-service secret — added to all Runtime calls.
    pub internal_service_token: String,
    /// TCP port this gateway listens on.
    pub port: u16,
    /// URL of the Runtime execution service.
    pub runtime_url: String,
    /// URL of the API (control-plane) service.
    #[allow(dead_code)]
    pub control_plane_url: String,
    /// Maximum HTTP request body in bytes before returning 413.
    pub max_request_size_bytes: usize,
    /// HTTP timeout for forwarded runtime calls (seconds).
    pub runtime_timeout_secs: u64,
    /// Per-route default rate limit (requests / second).
    pub rate_limit_per_sec: u32,
    /// When `true`, skip tenant resolution and inject a fixed dev identity.
    /// Set `LOCAL_MODE=true` (or `FLUX_LOCAL=true`).
    pub local_mode: bool,
}

impl Config {
    pub fn load() -> Self {
        Self {
            database_url: env::var("DATABASE_URL")
                .expect("DATABASE_URL is required"),

            internal_service_token: env::var("INTERNAL_SERVICE_TOKEN")
                .expect("INTERNAL_SERVICE_TOKEN is required"),

            port: env::var("PORT")
                .or_else(|_| env::var("GATEWAY_PORT"))
                .unwrap_or_else(|_| "8081".to_string())
                .parse()
                .expect("PORT must be a valid u16"),

            runtime_url: env::var("RUNTIME_URL")
                .unwrap_or_else(|_| "http://localhost:8083".to_string()),

            // Accept legacy API_URL for backward compat.
            control_plane_url: env::var("CONTROL_PLANE_URL")
                .or_else(|_| env::var("API_URL"))
                .unwrap_or_else(|_| "http://localhost:8080".to_string()),

            max_request_size_bytes: env::var("MAX_REQUEST_SIZE_BYTES")
                .ok().and_then(|s| s.parse().ok())
                .unwrap_or(10 * 1024 * 1024),

            runtime_timeout_secs: env::var("RUNTIME_TIMEOUT_SECS")
                .ok().and_then(|s| s.parse().ok())
                .unwrap_or(30),

            rate_limit_per_sec: env::var("RATE_LIMIT_PER_SEC")
                .ok().and_then(|s| s.parse().ok())
                .unwrap_or(50),

            // Accept "true", "1", "yes", "on" (case-insensitive).
            local_mode: env::var("LOCAL_MODE")
                .or_else(|_| env::var("FLUX_LOCAL"))
                .map(|v| matches!(v.to_lowercase().as_str(), "true" | "1" | "yes" | "on"))
                .unwrap_or(false),
        }
    }
}
