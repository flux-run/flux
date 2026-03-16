use std::net::SocketAddr;

use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio::sync::watch;
use tracing::info;

pub struct ServerConfig {
    pub grpc_port: u16,
    pub service_token: String,
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
                        "[Flux] INTERNAL_SERVICE_TOKEN must be set in production."
                    );
                }
                tracing::warn!(
                    "[Flux] INTERNAL_SERVICE_TOKEN not set — using insecure default 'dev-service-token'."
                );
                "dev-service-token".to_string()
            });

        Self { grpc_port, service_token }
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
        let pool = init_pool().await?;
        info!("Server connected to database");

        let (shutdown_tx, shutdown_rx) = watch::channel(());
        tokio::spawn(async move {
            shutdown_signal().await;
            info!("Shutdown signal received");
            let _ = shutdown_tx.send(());
        });

        let grpc_addr = SocketAddr::from(([0, 0, 0, 0], config.grpc_port));
        let grpc_service = crate::grpc::InternalAuthGrpc::new(pool, config.service_token);

        info!(port = config.grpc_port, "Flux server listening (gRPC)");
        crate::grpc::serve(grpc_addr, grpc_service, shutdown_rx).await?;

        Ok(())
    }
}

async fn init_pool() -> Result<PgPool, sqlx::Error> {
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    let max_connections = std::env::var("SERVER_DB_POOL_SIZE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(10);

    PgPoolOptions::new()
        .max_connections(max_connections)
        .after_connect(|conn, _meta| Box::pin(async move {
            sqlx::query("SET search_path = flux, public").execute(conn).await?;
            Ok(())
        }))
        .connect(&database_url)
        .await
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


