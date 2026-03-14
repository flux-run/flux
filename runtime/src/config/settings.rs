use std::env;

#[derive(Debug, Clone)]
pub struct Settings {
    /// Base URL of the API service — bundle fetch, secrets, log emission.
    /// e.g. `http://localhost:8080` in dev, internal Cloud Run URL in prod.
    pub api_url:         String,
    /// Base URL of the Queue service — used by ctx.queue.push() from user functions.
    pub queue_url:       String,
    pub service_token:   String,
    pub port:            u16,
    /// Number of V8 isolate worker threads.
    /// Defaults to 2× logical CPUs (min 2, max 16). Override with `ISOLATE_WORKERS`.
    pub isolate_workers: usize,
    /// Max simultaneous I/O-bound requests per V8 worker.
    /// When a worker reaches this limit it returns 503 until capacity frees.
    /// Override with `MAX_CONCURRENT_PER_WORKER`.
    pub max_concurrent_per_worker: usize,
    /// Per-request wall-clock timeout in seconds (V8 and WASM).
    /// Override with `REQUEST_TIMEOUT_SECONDS`.
    pub request_timeout_secs: u64,
    /// WASM CPU fuel limit (Wasmtime instruction units).
    /// 1 billion ≈ a few hundred ms of CPU. Override with `WASM_FUEL_LIMIT`.
    pub wasm_fuel_limit: u64,
}

impl Settings {
    pub fn load() -> Self {
        dotenvy::dotenv().ok();

        if env::var("RUST_LOG").is_err() {
            unsafe { env::set_var("RUST_LOG", "info,runtime=debug") };
        }
        let _ = tracing_subscriber::fmt::try_init();

        let api_url = env::var("API_URL")
            .or_else(|_| env::var("CONTROL_PLANE_URL"))
            .unwrap_or_else(|_| "http://localhost:8080".to_string());

        let queue_url = env::var("QUEUE_URL")
            .unwrap_or_else(|_| "http://localhost:8084".to_string());

        let service_token = env::var("SERVICE_TOKEN")
            .unwrap_or_else(|_| "stub_token".to_string());

        let port = env::var("PORT")
            .unwrap_or_else(|_| "8083".to_string())
            .parse()
            .unwrap_or(8083);

        let default_workers = std::thread::available_parallelism()
            .map(|n| (n.get() * 2).clamp(2, 16))
            .unwrap_or(4);
        let isolate_workers = env::var("ISOLATE_WORKERS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default_workers);

        let max_concurrent_per_worker = env::var("MAX_CONCURRENT_PER_WORKER")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(64);

        let request_timeout_secs = env::var("REQUEST_TIMEOUT_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        let wasm_fuel_limit = env::var("WASM_FUEL_LIMIT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1_000_000_000);

        Self {
            api_url,
            queue_url,
            service_token,
            port,
            isolate_workers,
            max_concurrent_per_worker,
            request_timeout_secs,
            wasm_fuel_limit,
        }
    }
}
