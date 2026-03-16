use std::net::SocketAddr;

use sha2::{Digest, Sha256};
use sqlx::PgPool;
use subtle::ConstantTimeEq;
use tokio::sync::watch;
use tonic::{Request, Response, Status};

pub mod pb {
    tonic::include_proto!("flux.internal.v1");
}

#[derive(Clone)]
pub struct InternalAuthGrpc {
    pool: PgPool,
    expected_token: String,
}

impl InternalAuthGrpc {
    pub fn new(pool: PgPool, expected_token: String) -> Self {
        Self { pool, expected_token }
    }

    async fn is_db_token_valid(&self, token: &str) -> Result<bool, sqlx::Error> {
        let token_hash = hex::encode(Sha256::digest(token.as_bytes()));

        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(\
               SELECT 1\
               FROM flux.service_tokens\
               WHERE token_hash = $1\
                 AND revoked_at IS NULL\
            )",
        )
        .bind(token_hash)
        .fetch_one(&self.pool)
        .await?;

        Ok(exists)
    }

    fn read_bearer_token(metadata: &tonic::metadata::MetadataMap) -> Option<String> {
        metadata
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    }

    async fn authenticate(
        &self,
        metadata: &tonic::metadata::MetadataMap,
    ) -> Result<String, Status> {
        let provided_token = Self::read_bearer_token(metadata).unwrap_or_default();

        if provided_token.is_empty() {
            return Err(Status::unauthenticated("missing authorization bearer token"));
        }

        if !self.expected_token.is_empty() {
            let env_match: bool = provided_token
                .as_bytes()
                .ct_eq(self.expected_token.as_bytes())
                .into();

            if env_match {
                return Ok("env".to_string());
            }
        }

        let db_match = self
            .is_db_token_valid(&provided_token)
            .await
            .map_err(|e| Status::internal(format!("token lookup failed: {e}")))?;

        if db_match {
            let token_hash = hex::encode(Sha256::digest(provided_token.as_bytes()));
            let pool = self.pool.clone();

            tokio::spawn(async move {
                let _ = sqlx::query(
                    "UPDATE flux.service_tokens\
                     SET last_used_at = now()\
                     WHERE token_hash = $1 AND revoked_at IS NULL",
                )
                .bind(token_hash)
                .execute(&pool)
                .await;
            });

            return Ok("db".to_string());
        }

        Err(Status::unauthenticated("invalid service token"))
    }
}

#[tonic::async_trait]
impl pb::internal_auth_service_server::InternalAuthService for InternalAuthGrpc {
    async fn validate_token(
        &self,
        request: Request<pb::ValidateTokenRequest>,
    ) -> Result<Response<pb::ValidateTokenResponse>, Status> {
        let auth_mode = self.authenticate(request.metadata()).await?;

        Ok(Response::new(pb::ValidateTokenResponse {
            ok: true,
            auth_mode,
        }))
    }

    async fn list_logs(
        &self,
        request: Request<pb::ListLogsRequest>,
    ) -> Result<Response<pb::ListLogsResponse>, Status> {
        let _auth_mode = self.authenticate(request.metadata()).await?;
        let limit = request.into_inner().limit.max(1).min(500) as i64;

        let rows: Vec<(String, String, String, Option<String>)> = sqlx::query_as(
            "SELECT
                request_id::text,
                code_sha,
                status,
                to_char(started_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')
             FROM flux.executions
             ORDER BY started_at DESC
             LIMIT $1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to list logs: {e}")))?;

        let logs = rows
            .into_iter()
            .map(|(request_id, code_version, status, timestamp)| pb::LogEntry {
                request_id,
                code_version,
                status,
                timestamp: timestamp.unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(pb::ListLogsResponse { logs }))
    }

    async fn record_execution(
        &self,
        request: Request<pb::RecordExecutionRequest>,
    ) -> Result<Response<pb::RecordExecutionResponse>, Status> {
        let _auth_mode = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();

        let execution_id = uuid::Uuid::parse_str(&req.execution_id)
            .map_err(|e| Status::invalid_argument(format!("invalid execution_id: {e}")))?;
        let request_id = uuid::Uuid::parse_str(&req.request_id)
            .map_err(|e| Status::invalid_argument(format!("invalid request_id: {e}")))?;

        let request_json: serde_json::Value = serde_json::from_str(&req.request_json)
            .map_err(|e| Status::invalid_argument(format!("invalid request_json: {e}")))?;
        let response_json: serde_json::Value = serde_json::from_str(&req.response_json)
            .map_err(|e| Status::invalid_argument(format!("invalid response_json: {e}")))?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Status::internal(format!("failed to begin transaction: {e}")))?;

        sqlx::query(
            "INSERT INTO flux.executions
             (id, request_id, method, path, status, request, response, error, code_sha, duration_ms)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NULLIF($8, ''), $9, $10)
             ON CONFLICT (id) DO UPDATE SET
               status = EXCLUDED.status,
               response = EXCLUDED.response,
               error = EXCLUDED.error,
               duration_ms = EXCLUDED.duration_ms",
        )
        .bind(execution_id)
        .bind(request_id)
        .bind(req.method)
        .bind(req.path)
        .bind(req.status)
        .bind(request_json)
        .bind(response_json)
        .bind(req.error)
        .bind(req.code_version)
        .bind(req.duration_ms)
        .execute(&mut *tx)
        .await
        .map_err(|e| Status::internal(format!("failed to upsert execution: {e}")))?;

        for checkpoint in req.checkpoints {
            let request_json: serde_json::Value = serde_json::from_str(&checkpoint.request_json)
                .unwrap_or(serde_json::Value::Null);
            let response_json: serde_json::Value = serde_json::from_str(&checkpoint.response_json)
                .unwrap_or(serde_json::Value::Null);

            sqlx::query(
                "INSERT INTO flux.checkpoints
                 (execution_id, call_index, boundary, url, method, request, response, duration_ms)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 ON CONFLICT (execution_id, call_index) DO UPDATE SET
                   response = EXCLUDED.response,
                   duration_ms = EXCLUDED.duration_ms",
            )
            .bind(execution_id)
            .bind(checkpoint.call_index as i32)
            .bind(checkpoint.boundary)
            .bind(checkpoint.url)
            .bind(checkpoint.method)
            .bind(request_json)
            .bind(response_json)
            .bind(checkpoint.duration_ms)
            .execute(&mut *tx)
            .await
            .map_err(|e| Status::internal(format!("failed to upsert checkpoint: {e}")))?;
        }

        tx.commit()
            .await
            .map_err(|e| Status::internal(format!("failed to commit execution: {e}")))?;

        Ok(Response::new(pb::RecordExecutionResponse { ok: true }))
    }
}

pub async fn serve(
    addr: SocketAddr,
    service: InternalAuthGrpc,
    mut shutdown_rx: watch::Receiver<()>,
) -> Result<(), tonic::transport::Error> {
    tonic::transport::Server::builder()
        .add_service(pb::internal_auth_service_server::InternalAuthServiceServer::new(service))
        .serve_with_shutdown(addr, async move {
            let _ = shutdown_rx.changed().await;
        })
        .await
}
