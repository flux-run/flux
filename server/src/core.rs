use std::net::SocketAddr;

use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio::sync::watch;
use tracing::info;

pub struct ServerConfig {
    pub grpc_port: u16,
    pub service_token: Option<String>,
}

impl ServerConfig {
    pub fn from_env() -> Self {
        let grpc_port = std::env::var("GRPC_PORT")
            .unwrap_or_else(|_| "50051".to_string())
            .parse::<u16>()
            .expect("GRPC_PORT must be a valid u16");

        let service_token = std::env::var("INTERNAL_SERVICE_TOKEN").ok();

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

        if let Some(token) = ensure_service_token(&pool).await? {
            println!();
            println!("Flux server started on :{}", config.grpc_port);
            println!();
            println!("Service token: {}", token);
            println!();
            println!("Store this token — it will not be shown again.");
            println!("Set it on your runtimes: flux serve index.js --token {}", token);
            println!();
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(());
        tokio::spawn(async move {
            shutdown_signal().await;
            info!("Shutdown signal received");
            let _ = shutdown_tx.send(());
        });

        let grpc_addr = SocketAddr::from(([0, 0, 0, 0], config.grpc_port));
        let grpc_service = crate::grpc::InternalAuthGrpc::new(
            pool,
            config.service_token.unwrap_or_default(),
        );

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

async fn ensure_service_token(pool: &PgPool) -> Result<Option<String>, sqlx::Error> {
    let existing_hash: Option<String> = sqlx::query_scalar(
        "SELECT token_hash FROM flux.service_tokens WHERE revoked_at IS NULL LIMIT 1",
    )
    .fetch_optional(pool)
    .await?;

    if existing_hash.is_some() {
        return Ok(None);
    }

    let raw = format!("flux_sk_{}", uuid::Uuid::new_v4().simple());
    let token_hash = hex::encode(Sha256::digest(raw.as_bytes()));

    sqlx::query(
        "INSERT INTO flux.service_tokens (service_name, token_hash) VALUES ('*', $1)",
    )
    .bind(token_hash)
    .execute(pool)
    .await?;

    Ok(Some(raw))
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


