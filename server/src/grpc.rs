use std::net::SocketAddr;

use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgListener};
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio_stream::wrappers::ReceiverStream;
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
    type TailStream = ReceiverStream<Result<pb::TailEvent, Status>>;

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
             VALUES ($1, $2, $3, $4, 'running', $5, NULL, NULL, $6, 0)
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(execution_id)
        .bind(request_id)
        .bind(req.method.clone())
        .bind(req.path.clone())
        .bind(request_json.clone())
        .bind(req.code_version.clone())
        .execute(&mut *tx)
        .await
        .map_err(|e| Status::internal(format!("failed to insert execution: {e}")))?;

        sqlx::query(
            "UPDATE flux.executions
             SET method = $2,
                 path = $3,
                 status = $4,
                 request = $5,
                 response = $6,
                 error = NULLIF($7, ''),
                 code_sha = $8,
                 duration_ms = $9
             WHERE id = $1",
        )
        .bind(execution_id)
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
        .map_err(|e| Status::internal(format!("failed to update execution: {e}")))?;

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

    async fn get_trace(
        &self,
        request: Request<pb::GetTraceRequest>,
    ) -> Result<Response<pb::GetTraceResponse>, Status> {
        let _auth_mode = self.authenticate(request.metadata()).await?;
        let execution_id_raw = request.into_inner().execution_id;
        let execution_id = uuid::Uuid::parse_str(&execution_id_raw)
            .map_err(|_| Status::invalid_argument("invalid execution_id"))?;

        let execution: Option<(String, String, String, i32, Option<String>)> = sqlx::query_as(
            "SELECT method, path, status, duration_ms, error
             FROM flux.executions
             WHERE id = $1",
        )
        .bind(execution_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch execution: {e}")))?;

        let (method, path, status, duration_ms, error) = execution
            .ok_or_else(|| Status::not_found("execution not found"))?;

        let checkpoint_rows: Vec<(i32, String, serde_json::Value, serde_json::Value, i32)> =
            sqlx::query_as(
                "SELECT call_index, boundary, request, response, duration_ms
                 FROM flux.checkpoints
                 WHERE execution_id = $1
                 ORDER BY call_index ASC",
            )
            .bind(execution_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("failed to fetch checkpoints: {e}")))?;

        let checkpoints = checkpoint_rows
            .into_iter()
            .map(|(call_index, boundary, request, response, duration_ms)| pb::Checkpoint {
                call_index,
                boundary,
                request: serde_json::to_vec(&request).unwrap_or_default(),
                response: serde_json::to_vec(&response).unwrap_or_default(),
                duration_ms,
            })
            .collect();

        Ok(Response::new(pb::GetTraceResponse {
            execution_id: execution_id_raw,
            method,
            path,
            status,
            duration_ms,
            error: error.unwrap_or_default(),
            checkpoints,
        }))
    }

    async fn tail(
        &self,
        request: Request<pb::TailRequest>,
    ) -> Result<Response<Self::TailStream>, Status> {
        let _auth_mode = self.authenticate(request.metadata()).await?;
        let pool = self.pool.clone();
        let project_id = request.into_inner().project_id;

        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            let mut listener = match PgListener::connect_with(&pool).await {
                Ok(listener) => listener,
                Err(err) => {
                    tracing::error!(error = %err, "tail listener connect failed");
                    return;
                }
            };

            if let Err(err) = listener.listen("flux_executions").await {
                tracing::error!(error = %err, "tail listener subscribe failed");
                return;
            }

            loop {
                match listener.recv().await {
                    Ok(notification) => {
                        let payload = notification.payload();
                        let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) else {
                            continue;
                        };

                        if !project_id.is_empty() {
                            let pid = val
                                .get("project_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("");
                            if pid != project_id {
                                continue;
                            }
                        }

                        let event = pb::TailEvent {
                            execution_id: val
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            method: val
                                .get("method")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            path: val
                                .get("path")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            status: val
                                .get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            duration_ms: val
                                .get("duration_ms")
                                .and_then(|v| v.as_i64())
                                .unwrap_or(0) as i32,
                            error: val
                                .get("error")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                            started_at: 0,
                        };

                        if tx.send(Ok(event)).await.is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::error!(error = %err, "tail listener recv failed");
                        break;
                    }
                }
            }
        });

        Ok(Response::new(ReceiverStream::new(rx)))
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
