use std::env;

#[derive(Debug, Clone)]
pub struct Settings {
    pub control_plane_url: String,
    pub service_token: String,
    pub port: u16,
    /// Number of isolate worker threads.  Defaults to 2× logical CPUs (min 2, max 16).
    /// Override with the `ISOLATE_WORKERS` env var.
    pub isolate_workers: usize,
}

impl Settings {
    pub fn load() -> Self {
        dotenvy::dotenv().ok();
        
        // Setup tracing
        if env::var("RUST_LOG").is_err() {
            unsafe { env::set_var("RUST_LOG", "info,runtime=debug") };
        }
        tracing_subscriber::fmt::init();
        
        let control_plane_url = env::var("CONTROL_PLANE_URL")
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

        Self {
            control_plane_url,
            service_token,
            port,
            isolate_workers,
        }
    }
}
