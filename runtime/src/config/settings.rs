use std::env;

#[derive(Debug, Clone)]
pub struct Settings {
    /// Base URL of the API service — bundle fetch, secrets, log emission.
    /// e.g. `http://localhost:8080` in dev, internal Cloud Run URL in prod.
    pub api_url:         String,
    pub service_token:   String,
    pub port:            u16,
    /// Number of V8 isolate worker threads.
    /// Defaults to 2× logical CPUs (min 2, max 16). Override with `ISOLATE_WORKERS`.
    pub isolate_workers: usize,
}

impl Settings {
    pub fn load() -> Self {
        dotenvy::dotenv().ok();

        if env::var("RUST_LOG").is_err() {
            unsafe { env::set_var("RUST_LOG", "info,runtime=debug") };
        }
        tracing_subscriber::fmt::init();

        let api_url = env::var("API_URL")
            .or_else(|_| env::var("CONTROL_PLANE_URL"))   // backward-compat alias
            .unwrap_or_else(|_| "http://localhost:8080".to_string());

        let service_token = env::var("SERVICE_TOKEN")
            .unwrap_or_else(|_| "stub_token".to_string());

        let port = env::var("PORT")
            .unwrap_or_else(|_| "8081".to_string())
            .parse()
            .unwrap_or(8081);

        let default_workers = std::thread::available_parallelism()
            .map(|n| (n.get() * 2).clamp(2, 16))
            .unwrap_or(4);
        let isolate_workers = env::var("ISOLATE_WORKERS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default_workers);

        Self { api_url, service_token, port, isolate_workers }
    }
}
