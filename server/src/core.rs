use std::net::SocketAddr;

use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgPoolOptions};
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

        Self {
            grpc_port,
            service_token,
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
        let pool = init_pool().await?;
        ensure_runtime_tables(&pool).await?;
        info!("Server connected to database");

        if let Some(token) = ensure_service_token(&pool).await? {
            println!();
            println!("Flux server started on :{}", config.grpc_port);
            println!();
            println!("Service token: {}", token);
            println!();
            println!("Store this token — it will not be shown again.");
            println!(
                "Set it on your runtimes: flux serve index.js --token {}",
                token
            );
            println!();
        }

        let (shutdown_tx, shutdown_rx) = watch::channel(());
        tokio::spawn(async move {
            shutdown_signal().await;
            info!("Shutdown signal received");
            let _ = shutdown_tx.send(());
        });

        let grpc_addr = SocketAddr::from(([0, 0, 0, 0], config.grpc_port));
        let grpc_service =
            crate::grpc::InternalAuthGrpc::new(pool, config.service_token.unwrap_or_default());

        info!(port = config.grpc_port, "Flux server listening (gRPC)");
        crate::grpc::serve(grpc_addr, grpc_service, shutdown_rx).await?;

        Ok(())
    }
}

async fn init_pool() -> Result<PgPool, Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            "DATABASE_URL environment variable is not set. Please provide it via the --database-url flag or export it."
        })?;

    let max_connections = std::env::var("SERVER_DB_POOL_SIZE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(10);

    PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(&database_url)
        .await
        .map_err(|err| {
            format!(
                "Failed to connect to Postgres: {}. Please check your DATABASE_URL and ensure the database is running.",
                err
            ).into()
        })
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

    sqlx::query("INSERT INTO flux.service_tokens (service_name, token_hash) VALUES ('*', $1)")
        .bind(token_hash)
        .execute(pool)
        .await?;

    Ok(Some(raw))
}

async fn ensure_runtime_tables(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("CREATE SCHEMA IF NOT EXISTS flux")
        .execute(pool)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS flux.service_tokens (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            service_name TEXT NOT NULL,
            token_hash TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            revoked_at TIMESTAMPTZ
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS flux.executions (
            id UUID PRIMARY KEY,
            request_id UUID NOT NULL,
            project_id TEXT,
            org_id TEXT,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            status TEXT NOT NULL,
            request JSONB,
            response JSONB,
            error TEXT,
            code_sha TEXT NOT NULL,
            started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            duration_ms INTEGER NOT NULL DEFAULT 0
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS flux.checkpoints (
            execution_id UUID NOT NULL,
            call_index INTEGER NOT NULL,
            org_id TEXT,
            boundary TEXT NOT NULL,
            url TEXT,
            method TEXT,
            request JSONB,
            response JSONB,
            duration_ms INTEGER NOT NULL DEFAULT 0,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY (execution_id, call_index)
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS flux.execution_console_logs (
            execution_id UUID NOT NULL,
            seq INTEGER NOT NULL,
            org_id TEXT,
            level TEXT NOT NULL,
            message TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY (execution_id, seq)
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE OR REPLACE FUNCTION flux.notify_execution()
         RETURNS trigger AS $$
         BEGIN
             RAISE NOTICE 'Trigger firing status=% id=%', NEW.status, NEW.id;
             IF (TG_OP = 'UPDATE' AND NEW.status IN ('ok', 'error', 'timeout')) THEN
                 PERFORM pg_notify('flux_executions', json_build_object(
                     'id', NEW.id,
                     'org_id', NEW.org_id,
                     'project_id', NEW.project_id,
                     'method', NEW.method,
                     'path', NEW.path,
                     'status', NEW.status,
                     'duration_ms', NEW.duration_ms,
                     'error', NEW.error
                 )::text);
             END IF;
             RETURN NEW;
         END;
         $$ LANGUAGE plpgsql"#,
    )
    .execute(pool)
    .await?;

    sqlx::query("DROP TRIGGER IF EXISTS trg_execution_notify ON flux.executions")
        .execute(pool)
        .await?;

    sqlx::query(
        "CREATE TRIGGER trg_execution_notify
         AFTER INSERT OR UPDATE ON flux.executions
         FOR EACH ROW EXECUTE FUNCTION flux.notify_execution()",
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE SCHEMA IF NOT EXISTS control")
        .execute(pool)
        .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS control.projects (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            org_id UUID,
            name TEXT NOT NULL,
            base_domain TEXT,
            created_at TIMESTAMPTZ DEFAULT now()
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS control.functions (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            project_id UUID NOT NULL REFERENCES control.projects(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            latest_artifact_id TEXT,
            created_at TIMESTAMPTZ DEFAULT now(),
            UNIQUE(project_id, name)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS control.routes (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            project_id UUID NOT NULL REFERENCES control.projects(id) ON DELETE CASCADE,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            function_id UUID REFERENCES control.functions(id) ON DELETE CASCADE,
            created_at TIMESTAMPTZ DEFAULT now(),
            UNIQUE(project_id, method, path)
        )",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS control.env_vars (
            project_id UUID NOT NULL REFERENCES control.projects(id) ON DELETE CASCADE,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY (project_id, key)
        )",
    )
    .execute(pool)
    .await?;

    // Performance indexes — idempotent with IF NOT EXISTS.
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_executions_started_at ON flux.executions (started_at DESC)",
    )
    .execute(pool)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_executions_status_started ON flux.executions (status, started_at DESC)")
        .execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_executions_path_started ON flux.executions (path, started_at DESC)")
        .execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_checkpoints_execution_call ON flux.checkpoints (execution_id, call_index)")
        .execute(pool).await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_console_logs_execution_seq ON flux.execution_console_logs (execution_id, seq)")
        .execute(pool).await?;

    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");

        tokio::select! {
            _ = ctrl_c         => {}
            _ = sigterm.recv() => {}
        }
    }

    #[cfg(not(unix))]
    ctrl_c.await;
}
