use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;
use tracing::info;

use dispatch::{InProcessApiDispatch, InProcessDataEngineDispatch, InProcessQueueDispatch, InProcessRuntimeDispatch};
use job_contract::dispatch::{ApiDispatch, DataEngineDispatch, QueueDispatch, RuntimeDispatch};
use runtime::bundle::cache::BundleCache;
use runtime::engine::executor::PoolDispatchers;
use runtime::engine::pool::IsolatePool;
use runtime::schema::cache::SchemaCache;
use runtime::secrets::client::SecretsClient;

use crate::dispatch;

pub struct ServerConfig {
    pub grpc_port: u16,
    pub service_token: String,
    pub isolate_workers: usize,
    pub queue_worker_concurrency: usize,
    pub queue_poll_interval_ms: u64,
    pub queue_timeout_check_ms: u64,
    pub request_timeout_secs: u64,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let grpc_port = std::env::var("GRPC_PORT")
            .unwrap_or_else(|_| "50051".to_string())
            .parse::<u16>()
            .expect("GRPC_PORT must be a valid u16");

        let service_token = std::env::var("INTERNAL_SERVICE_TOKEN")
            .unwrap_or_else(|_| {
                if std::env::var("FLUX_ENV").as_deref() == Ok("production") {
                    panic!(
                        "[Flux] INTERNAL_SERVICE_TOKEN must be set in production. \
                         The server cannot start without it."
                    );
                }
                tracing::warn!(
                    "[Flux] INTERNAL_SERVICE_TOKEN not set — using insecure default 'dev-service-token'. \
                     Set INTERNAL_SERVICE_TOKEN in production."
                );
                "dev-service-token".to_string()
            });

        let isolate_workers = std::env::var("ISOLATE_WORKERS")
            .unwrap_or_else(|_| "4".to_string())
            .parse::<usize>()
            .unwrap_or(4);

        let queue_worker_concurrency: usize = std::env::var("QUEUE_WORKER_CONCURRENCY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let queue_poll_interval_ms: u64 = std::env::var("WORKER_POLL_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(200);

        let queue_timeout_check_ms: u64 = std::env::var("JOB_TIMEOUT_CHECK_INTERVAL_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30_000);

        let request_timeout_secs: u64 = std::env::var("REQUEST_TIMEOUT_SECONDS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);

        Self {
            grpc_port,
            service_token,
            isolate_workers,
            queue_worker_concurrency,
            queue_poll_interval_ms,
            queue_timeout_check_ms,
            request_timeout_secs,
        }
    }
}

pub struct CoreService {
    config: ServerConfig,
}

impl CoreService {
    pub fn new(config: ServerConfig) -> Self {
        Self { config }
    }

    pub async fn run(self) -> Result<(), Box<dyn std::error::Error>> {
        let config = self.config;

        let http_client = reqwest::Client::builder()
            .pool_max_idle_per_host(4)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(30))
            .build()?;

        api::config::init();
        let pool = api::db::connection::init_pool().await?;
        info!("Server connected to database (flux pool)");

        let (shutdown_tx, shutdown_rx) = watch::channel(());

        let shutdown_tx_clone = shutdown_tx.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            info!("Shutdown signal received — stopping background workers");
            let _ = shutdown_tx_clone.send(());
        });

        let grpc_addr = SocketAddr::from(([0, 0, 0, 0], config.grpc_port));
        let grpc_service = crate::grpc::InternalAuthGrpc::new(
            pool.clone(),
            config.service_token.clone(),
        );
        let grpc_shutdown_rx = shutdown_rx.clone();

        let base_url = format!("grpc://localhost:{}", config.grpc_port);
        let api_state = Arc::new(api::AppState {
            pool: pool.clone(),
            http_client: http_client.clone(),
            data_engine_url: format!("{}/flux/data-engine", base_url),
            gateway_url: base_url.clone(),
            runtime_url: base_url.clone(),
            functions_dir: std::env::var("FLUX_FUNCTIONS_DIR")
                .unwrap_or_else(|_| "./flux-functions".to_string()),
        });

        let api_dispatch_inproc: Arc<dyn ApiDispatch> = Arc::new(InProcessApiDispatch {
            state: Arc::clone(&api_state),
        });

        let api_dispatch_runtime: Arc<dyn ApiDispatch> = Arc::clone(&api_dispatch_inproc);

        let api_dispatch_for_queue = Arc::clone(&api_dispatch_inproc);
        let api_dispatch_for_worker = Arc::clone(&api_dispatch_inproc);

        let queue_dispatch: Arc<dyn QueueDispatch> = Arc::new(InProcessQueueDispatch {
            pool: pool.clone(),
        });
        let de_dispatch: Arc<dyn DataEngineDispatch> = Arc::new(InProcessDataEngineDispatch::new(
            pool.clone(),
            env_parse("STATEMENT_TIMEOUT_MS", 5000),
        ));

        let runtime_lock = Arc::new(std::sync::OnceLock::new());
        let dispatchers = PoolDispatchers {
            api: Arc::clone(&api_dispatch_runtime),
            queue: Arc::clone(&queue_dispatch),
            data_engine: Arc::clone(&de_dispatch),
            runtime: Arc::clone(&runtime_lock),
        };

        let runtime_state = Arc::new(runtime::AppState {
            secrets_client: SecretsClient::new(Arc::clone(&api_dispatch_runtime)),
            http_client: http_client.clone(),
            api: api_dispatch_runtime,
            queue: queue_dispatch,
            data_engine: de_dispatch,
            service_token: config.service_token.clone(),
            bundle_cache: BundleCache::new(100),
            schema_cache: SchemaCache::new(200),
            isolate_pool: IsolatePool::new(config.isolate_workers, config.request_timeout_secs, dispatchers.clone()),
            dispatchers: dispatchers.clone(),
        });

        let runtime_dispatch_inproc = Arc::new(InProcessRuntimeDispatch {
            state: Arc::clone(&runtime_state),
        });
        let _ = runtime_lock.set(
            Arc::clone(&runtime_dispatch_inproc) as Arc<dyn RuntimeDispatch>,
        );
        let runtime_dispatch_ref: Arc<dyn RuntimeDispatch> =
            Arc::clone(&runtime_dispatch_inproc) as Arc<dyn RuntimeDispatch>;

        let queue_state = Arc::new(
            flux_queue::state::AppState::new(
                pool.clone(),
                api_dispatch_for_queue,
            ),
        );

        let runtime_dispatch_for_worker: Arc<dyn RuntimeDispatch> = Arc::clone(&runtime_dispatch_ref);

        tokio::spawn(flux_queue::worker::worker::start(
            pool.clone(),
            api_dispatch_for_worker,
            runtime_dispatch_for_worker,
            config.service_token.clone(),
            config.queue_worker_concurrency,
            config.queue_poll_interval_ms,
            shutdown_rx.clone(),
        ));
        info!("Queue worker started (concurrency={})", config.queue_worker_concurrency);

        tokio::spawn(flux_queue::worker::timeout_recovery::run(
            pool.clone(),
            config.queue_timeout_check_ms,
            shutdown_rx.clone(),
        ));

        tokio::spawn(api::routes::monitor::run_alert_evaluator(pool.clone()));
        info!("Monitor alert evaluator started");

        let _ = runtime_state;
        let _ = runtime_dispatch_ref;
        let _ = queue_state;
        let _ = shutdown_tx;

        info!(port = config.grpc_port, "Flux core service listening (gRPC-only)");
        crate::grpc::serve(grpc_addr, grpc_service, grpc_shutdown_rx).await?;

        Ok(())
    }
}

fn env_parse<T: std::str::FromStr>(key: &str, default: T) -> T {
    std::env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(
            tokio::signal::unix::SignalKind::terminate(),
        )
        .expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = ctrl_c         => {}
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await;
}
