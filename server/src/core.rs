use std::net::SocketAddr;
use std::time::{SystemTime, UNIX_EPOCH};

use sha2::{Digest, Sha256};
use sqlx::{postgres::PgPoolOptions, PgPool};
use tokio::sync::watch;
use tokio::time::{interval, Duration};
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

        let service_token = std::env::var("FLUX_SERVICE_TOKEN").ok();

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
        spawn_request_reconciler(pool.clone());
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

fn spawn_request_reconciler(pool: PgPool) {
    let unknown_after_secs = std::env::var("FLUX_REQUEST_UNKNOWN_AFTER_SECS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(15);

    // Executions stuck in 'starting' (claimed but runtime crashed before RecordExecution)
    // are reaped after this threshold and marked as failed.
    let starting_timeout_secs = std::env::var("FLUX_STARTING_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(30);

    tokio::spawn(async move {
        let mut ticker = interval(Duration::from_secs(5));
        loop {
            ticker.tick().await;

            let jitter_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|duration| (duration.subsec_nanos() % 6) as i64)
                .unwrap_or(0);
            let reap_threshold_secs = starting_timeout_secs.saturating_add(jitter_secs);

            let stale_request_ids: Vec<uuid::Uuid> = match sqlx::query_scalar(
                "UPDATE flux.requests \
                 SET status = 'unknown', \
                     updated_at = now(), \
                     note = COALESCE(note, 'runtime did not acknowledge within threshold') \
                 WHERE status = 'dispatched' \
                   AND dispatched_at IS NOT NULL \
                   AND dispatched_at < now() - ($1::bigint * interval '1 second') \
                 RETURNING id",
            )
            .bind(unknown_after_secs)
            .fetch_all(&pool)
            .await
            {
                Ok(ids) => ids,
                Err(error) => {
                    tracing::warn!(error = %error, "request reconciler failed to mark stale dispatched requests");
                    continue;
                }
            };

            for request_id in stale_request_ids {
                if let Err(error) = sqlx::query(
                    "INSERT INTO flux.execution_events (request_id, step, status, metadata) \
                     VALUES ($1, 'timeout', 'unknown', $2)",
                )
                .bind(request_id)
                .bind(serde_json::json!({ "source": "request_reconciler" }))
                .execute(&pool)
                .await
                {
                    tracing::warn!(request_id = %request_id, error = %error, "failed to append timeout reconciliation event");
                }
            }

            // Reap executions claimed but never started: runtime crashed between claim and
            // the first RecordExecution call. Mark them 'failed' so the request can be retried.
            // Guard: only reap if no younger attempt exists for the same request — prevents
            // overwriting a completed retry that raced in before the reconciler ran.
            let stale_executions: Vec<(uuid::Uuid, uuid::Uuid)> = match sqlx::query_as(
                                "WITH candidates AS ( \
                                         SELECT e.id \
                                         FROM flux.executions e \
                                         WHERE e.status = 'starting' \
                                             AND e.created_at < now() - ($1::bigint * interval '1 second') \
                                             AND NOT EXISTS ( \
                                                     SELECT 1 FROM flux.executions e2 \
                                                     WHERE e2.request_id = e.request_id \
                                                         AND e2.attempt > e.attempt \
                                             ) \
                                         ORDER BY e.created_at \
                                         LIMIT 500 \
                                 ) \
                                 UPDATE flux.executions e \
                                 SET status = 'failed' \
                                 FROM candidates c \
                                 WHERE e.id = c.id \
                                 RETURNING e.id, e.request_id",
            )
                        .bind(reap_threshold_secs)
            .fetch_all(&pool)
            .await
            {
                Ok(rows) => rows,
                Err(error) => {
                    tracing::warn!(error = %error, "reconciler failed to reap stale starting executions");
                    continue;
                }
            };

            for (execution_id, request_id) in stale_executions {
                tracing::warn!(
                    execution_id = %execution_id,
                    request_id = %request_id,
                    "reconciler reaped execution stuck in 'starting' — runtime likely crashed after claim"
                );
                if let Err(error) = sqlx::query(
                    "INSERT INTO flux.execution_events (request_id, step, status, metadata) \
                     VALUES ($1, 'claim_abandoned', 'failed', $2)",
                )
                .bind(request_id)
                .bind(serde_json::json!({ "source": "request_reconciler", "execution_id": execution_id.to_string() }))
                .execute(&pool)
                .await
                {
                    tracing::warn!(execution_id = %execution_id, error = %error, "failed to append claim_abandoned event");
                }
            }
        }
    });
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
            attempt INT NOT NULL DEFAULT 1,
            parent_execution_id UUID,
            project_id TEXT,
            org_id TEXT,
            token_id UUID,
            method TEXT NOT NULL,
            path TEXT NOT NULL,
            status TEXT NOT NULL,
            request JSONB,
            response JSONB,
            request_method TEXT,
            request_headers JSONB,
            request_body TEXT,
            response_status INT,
            response_body TEXT,
            client_ip TEXT,
            user_agent TEXT,
            error TEXT,
            error_name TEXT,
            error_message TEXT,
            error_stack TEXT,
            error_fingerprint TEXT,
            error_phase TEXT,
            is_user_code BOOLEAN,
            error_source TEXT,
            error_type TEXT,
            code_sha TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            duration_ms INTEGER NOT NULL DEFAULT 0
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS flux.requests (
            id UUID PRIMARY KEY,
            project_id UUID,
            route TEXT NOT NULL,
            method TEXT NOT NULL,
            status TEXT NOT NULL,
            received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            dispatched_at TIMESTAMPTZ,
            started_at TIMESTAMPTZ,
            completed_at TIMESTAMPTZ,
            duration_ms INTEGER,
            next_attempt INT NOT NULL DEFAULT 2,
            retry_count INT NOT NULL DEFAULT 0,
            last_attempt_at TIMESTAMPTZ,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            ingestion_source TEXT NOT NULL,
            note TEXT
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query("ALTER TABLE flux.requests ADD COLUMN IF NOT EXISTS dispatched_at TIMESTAMPTZ")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.requests ADD COLUMN IF NOT EXISTS next_attempt INT NOT NULL DEFAULT 2")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.requests ADD COLUMN IF NOT EXISTS retry_count INT NOT NULL DEFAULT 0")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.requests ADD COLUMN IF NOT EXISTS last_attempt_at TIMESTAMPTZ")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.requests ADD COLUMN IF NOT EXISTS updated_at TIMESTAMPTZ NOT NULL DEFAULT now()")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS flux.execution_events (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            request_id UUID NOT NULL REFERENCES flux.requests(id) ON DELETE CASCADE,
            step TEXT NOT NULL,
            status TEXT,
            metadata JSONB,
            timestamp TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_flux_requests_project_received ON flux.requests(project_id, received_at DESC)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_flux_requests_status_updated ON flux.requests(status, updated_at DESC)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_flux_requests_dispatched_at ON flux.requests(dispatched_at)")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS attempt INT NOT NULL DEFAULT 1")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS parent_execution_id UUID")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS created_at TIMESTAMPTZ NOT NULL DEFAULT now()")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_flux_executions_request_id ON flux.executions(request_id)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_flux_executions_status ON flux.executions(status)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE UNIQUE INDEX IF NOT EXISTS uniq_flux_executions_request_attempt ON flux.executions(request_id, attempt)")
        .execute(pool)
        .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_flux_execution_events_request_ts ON flux.execution_events(request_id, timestamp ASC)")
        .execute(pool)
        .await?;
    sqlx::query(
        r#"CREATE OR REPLACE VIEW flux.latest_executions AS
           SELECT DISTINCT ON (request_id) *
           FROM flux.executions
           ORDER BY request_id, attempt DESC, created_at DESC"#,
    )
    .execute(pool)
    .await?;

    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS error_name TEXT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS error_message TEXT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS error_phase TEXT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS is_user_code BOOLEAN")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS error_frames JSONB")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS function_id UUID")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS failure_point_file TEXT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS failure_point_line INT")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS aborted BOOLEAN")
        .execute(pool)
        .await?;
    sqlx::query("ALTER TABLE flux.executions ADD COLUMN IF NOT EXISTS response_sent BOOLEAN")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS flux.spans (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            execution_id UUID NOT NULL REFERENCES flux.executions(id) ON DELETE CASCADE,
            type TEXT NOT NULL,
            label TEXT,
            start_ms INTEGER NOT NULL,
            duration_ms INTEGER NOT NULL,
            metadata JSONB
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS flux.issues (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            function_id UUID NOT NULL REFERENCES flux.functions(id) ON DELETE CASCADE,
            fingerprint TEXT NOT NULL,
            title TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'open',
            occurrence_count INTEGER NOT NULL DEFAULT 1,
            unique_ips INTEGER NOT NULL DEFAULT 1,
            unique_tokens INTEGER NOT NULL DEFAULT 0,
            sample_execution_id UUID,
            sample_stack TEXT,
            sample_message TEXT,
            first_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
            last_seen TIMESTAMPTZ NOT NULL DEFAULT now(),
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            UNIQUE(function_id, fingerprint)
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
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_executions_function_started ON flux.executions (function_id, started_at DESC) WHERE function_id IS NOT NULL")
        .execute(pool).await?;

    // Deployment lifecycle tracking — one record per artifact upload + each boot attempt.
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS control.deployments (
            id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
            function_id UUID NOT NULL REFERENCES control.functions(id) ON DELETE CASCADE,
            artifact_id TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'deployed',
            error_type TEXT,
            error_message TEXT,
            error_detail JSONB,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "CREATE INDEX IF NOT EXISTS idx_deployments_function_created \
         ON control.deployments (function_id, created_at DESC)",
    )
    .execute(pool)
    .await?;

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
