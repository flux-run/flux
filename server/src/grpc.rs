use std::net::SocketAddr;
use std::collections::HashMap;

use reqwest::Client;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgListener};
use subtle::ConstantTimeEq;
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub use shared::pb;

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

#[derive(Debug, Clone)]
struct WhyExecution {
    status: String,
    duration_ms: i32,
    error: Option<String>,
}

#[derive(Debug, Clone)]
struct WhyCheckpoint {
    call_index: i32,
    boundary: String,
    request: serde_json::Value,
    response: serde_json::Value,
    duration_ms: i32,
}

fn analyze_execution(exec: &WhyExecution, checkpoints: &[WhyCheckpoint]) -> (String, String) {
    if exec.status == "error" {
        if let Some(last) = checkpoints.last() {
            if last.boundary == "http" {
                let status = last
                    .response
                    .get("status")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0);
                let url = last
                    .request
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                if status >= 500 {
                    return (
                        format!(
                            "external service error\ncall    {} {}\nstatus  {}\nindex   {}",
                            last.boundary.to_uppercase(),
                            url,
                            status,
                            last.call_index
                        ),
                        "the upstream service returned a 5xx — not a bug in your code".to_string(),
                    );
                }

                if status == 429 {
                    return (
                        format!("rate limited\ncall    {}", url),
                        "add retry with exponential backoff".to_string(),
                    );
                }

                if status == 401 || status == 403 {
                    return (
                        format!("auth failure\ncall    {}\nstatus  {}", url, status),
                        "check credentials/token for this service".to_string(),
                    );
                }

                if status == 0 {
                    return (
                        format!("network failure — no response received\ncall    {}", url),
                        "check connectivity or add timeout handling".to_string(),
                    );
                }
            }

            if last.boundary == "db" {
                let query = last
                    .request
                    .get("query")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown query");
                let error = exec.error.as_deref().unwrap_or("");

                if error.contains("duplicate key") || error.contains("unique") {
                    return (
                        format!(
                            "duplicate key violation\nquery   {}\nerror   {}",
                            query, error
                        ),
                        "check for existing record before inserting".to_string(),
                    );
                }

                if error.contains("foreign key") {
                    return (
                        format!("foreign key violation\nquery   {}", query),
                        "ensure referenced record exists first".to_string(),
                    );
                }

                if error.contains("null") || error.contains("not-null") {
                    return (
                        format!("null constraint violation\nquery   {}", query),
                        "check required fields are present in input".to_string(),
                    );
                }

                return (
                    format!("database error\nquery   {}\nerror   {}", query, error),
                    String::new(),
                );
            }
        }

        return (
            format!(
                "function threw before any IO\nerror   {}",
                exec.error.as_deref().unwrap_or("unknown error")
            ),
            "check input validation and early-exit logic".to_string(),
        );
    }

    if exec.duration_ms > 1000 {
        if let Some(slow) = checkpoints.iter().max_by_key(|c| c.duration_ms) {
            let url = slow
                .request
                .get("url")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            return (
                format!(
                    "slow execution — {}ms total\nslowest call   [{}] {}  {}ms",
                    exec.duration_ms, slow.call_index, url, slow.duration_ms
                ),
                "consider caching or parallelising this call".to_string(),
            );
        }
    }

    (
        format!(
            "no issues found\nstatus   {}\nduration {}ms\ncalls    {}",
            exec.status,
            exec.duration_ms,
            checkpoints.len()
        ),
        String::new(),
    )
}

async fn perform_live_http_call(request: &serde_json::Value) -> Result<(serde_json::Value, i32), Status> {
    let url = request
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if url.is_empty() {
        return Err(Status::invalid_argument("resume: checkpoint request missing url"));
    }

    let method = request
        .get("method")
        .and_then(|v| v.as_str())
        .unwrap_or("GET")
        .parse::<reqwest::Method>()
        .map_err(|e| Status::invalid_argument(format!("resume: invalid method: {e}")))?;

    let client = Client::new();
    let mut req = client.request(method, url);

    if let Some(headers_val) = request.get("headers") {
        if !headers_val.is_null() {
            let map: HashMap<String, String> = serde_json::from_value(headers_val.clone())
                .map_err(|e| Status::invalid_argument(format!("resume: invalid headers: {e}")))?;
            for (k, v) in map {
                req = req.header(k, v);
            }
        }
    }

    if let Some(body_val) = request.get("body") {
        if !body_val.is_null() {
            req = req.json(body_val);
        }
    }

    let started = std::time::Instant::now();
    let resp = req
        .send()
        .await
        .map_err(|e| Status::internal(format!("resume: live HTTP call failed: {e}")))?;
    let duration_ms = started.elapsed().as_millis() as i32;

    let status = resp.status().as_u16();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or_default().to_string()))
        .collect::<HashMap<_, _>>();

    let body_text = resp
        .text()
        .await
        .map_err(|e| Status::internal(format!("resume: failed to read response body: {e}")))?;
    let parsed_body = serde_json::from_str::<serde_json::Value>(&body_text)
        .unwrap_or_else(|_| serde_json::Value::String(body_text));

    Ok((
        serde_json::json!({
            "status": status,
            "headers": headers,
            "body": parsed_body,
        }),
        duration_ms,
    ))
}

fn summarize_http_response(value: &serde_json::Value) -> String {
    let status = value
        .get("status")
        .and_then(|v| v.as_i64())
        .map(|v| v.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let body = value.get("body").cloned().unwrap_or(serde_json::Value::Null);
    let body_text = serde_json::to_string(&body).unwrap_or_else(|_| "null".to_string());
    let shortened_body = if body_text.len() > 160 {
        format!("{}...", &body_text[..160])
    } else {
        body_text
    };

    format!("status={} body={}", status, shortened_body)
}

fn compact_json(value: &serde_json::Value) -> String {
    let rendered = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    if rendered.len() > 240 {
        format!("{}...", &rendered[..240])
    } else {
        rendered
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

        let rows: Vec<(String, String, String, String, String, i32, Option<String>, Option<String>, Option<String>)> = sqlx::query_as(
            "SELECT
                id::text,
                request_id::text,
                method,
                path,
                status,
                duration_ms,
                code_sha,
                error,
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
            .map(|(execution_id, request_id, method, path, status, duration_ms, code_version, error, timestamp)| pb::LogEntry {
                execution_id,
                request_id,
                method,
                path,
                code_version: code_version.unwrap_or_default(),
                status,
                duration_ms,
                error: error.unwrap_or_default(),
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

        let execution: Option<(String, String, String, i32, Option<String>, serde_json::Value, serde_json::Value)> = sqlx::query_as(
            "SELECT method, path, status, duration_ms, error, request, response
             FROM flux.executions
             WHERE id = $1",
        )
        .bind(execution_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch execution: {e}")))?;

        let (method, path, status, duration_ms, error, request_json, response_json) = execution
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
            request_json: serde_json::to_string(&request_json).unwrap_or_else(|_| "null".to_string()),
            response_json: serde_json::to_string(&response_json).unwrap_or_else(|_| "null".to_string()),
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

    async fn why(
        &self,
        request: Request<pb::WhyRequest>,
    ) -> Result<Response<pb::WhyResponse>, Status> {
        let _auth_mode = self.authenticate(request.metadata()).await?;
        let execution_id_raw = request.into_inner().execution_id;
        let execution_id = uuid::Uuid::parse_str(&execution_id_raw)
            .map_err(|_| Status::invalid_argument("invalid execution_id"))?;

        let execution: Option<(String, i32, Option<String>)> = sqlx::query_as(
            "SELECT status, duration_ms, error
             FROM flux.executions
             WHERE id = $1",
        )
        .bind(execution_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch execution: {e}")))?;

        let (status, duration_ms, error) = execution
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

        let checkpoints: Vec<WhyCheckpoint> = checkpoint_rows
            .into_iter()
            .map(|(call_index, boundary, request, response, duration_ms)| WhyCheckpoint {
                call_index,
                boundary,
                request,
                response,
                duration_ms,
            })
            .collect();

        let (reason, suggestion) = analyze_execution(
            &WhyExecution {
                status,
                duration_ms,
                error,
            },
            &checkpoints,
        );

        Ok(Response::new(pb::WhyResponse {
            execution_id: execution_id_raw,
            reason,
            suggestion,
        }))
    }

    async fn replay(
        &self,
        request: Request<pb::ReplayRequest>,
    ) -> Result<Response<pb::ReplayResponse>, Status> {
        let _auth_mode = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let commit = req.commit;
        let validate = req.validate;

        if validate && !commit {
            return Err(Status::invalid_argument(
                "replay validation requires commit mode so live checkpoint results can be compared",
            ));
        }

        let source_execution_id = uuid::Uuid::parse_str(&req.execution_id)
            .map_err(|_| Status::invalid_argument("invalid execution_id"))?;
        let from_index = req.from_index.max(0);

        // 1. Fetch original execution
        let source_execution: Option<(
            String,
            String,
            serde_json::Value,
            Option<serde_json::Value>,
            String,
            Option<String>,
            i32,
            String,
        )> = sqlx::query_as(
            "SELECT method, path, request, response, status, error, duration_ms, code_sha
             FROM flux.executions
             WHERE id = $1",
        )
        .bind(source_execution_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch source execution: {e}")))?;

        let (method, path, request_json, _response_json, _status, _error, _duration_ms, code_sha) =
            source_execution.ok_or_else(|| Status::not_found("execution not found"))?;

        // 2. Fetch all checkpoints from the original execution
        let checkpoint_rows: Vec<(
            i32,
            String,
            Option<String>,
            Option<String>,
            serde_json::Value,
            serde_json::Value,
            i32,
        )> = sqlx::query_as(
            "SELECT call_index, boundary, url, method, request, response, duration_ms
             FROM flux.checkpoints
             WHERE execution_id = $1 AND call_index >= $2
             ORDER BY call_index ASC",
        )
        .bind(source_execution_id)
        .bind(from_index)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch checkpoints: {e}")))?;

        // 3. Re-execute through the runtime with recorded checkpoints injected.
        //    The runtime's Replay mode returns recorded responses from op_fetch
        //    instead of making live HTTP calls, so the JS runs the same code path
        //    but with deterministic external responses.
        //
        //    We send the replay request to the runtime HTTP endpoint with the
        //    original request body. The runtime re-runs the JS, op_fetch returns
        //    the recorded checkpoint responses, and we get a fresh execution result.
        //
        //    For now, since the server doesn't hold a direct reference to the
        //    runtime isolate pool, replay works by recording the original checkpoint
        //    responses as the replay result. When the server and runtime are
        //    in-process (single binary), this will call the isolate pool directly.

        let replay_execution_id = uuid::Uuid::new_v4();
        let replay_request_id = uuid::Uuid::new_v4();

        let replay_started = std::time::Instant::now();

        // Re-execute each checkpoint: for HTTP boundaries, make a live call to
        // compare against the recorded result. For non-HTTP boundaries, carry
        // forward the recorded data.
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Status::internal(format!("failed to begin replay transaction: {e}")))?;

        let mut steps = Vec::with_capacity(checkpoint_rows.len());
        let mut replay_status = "ok".to_string();
        let mut replay_error = String::new();
        let mut replay_output = serde_json::Value::Null;
        let mut divergence: Option<pb::ReplayDivergence> = None;

        for (call_index, boundary, url, cp_method, cp_request, cp_response, checkpoint_duration_ms) in
            &checkpoint_rows
        {
            let step_url = url.clone().unwrap_or_else(|| {
                cp_request
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string()
            });

            // If commit is true, re-execute HTTP calls live; otherwise use recorded.
            let (response_to_store, step_duration, used_recorded) = if commit && boundary == "http" {
                match perform_live_http_call(cp_request).await {
                    Ok((live_resp, dur)) => (live_resp, dur, false),
                    Err(e) => {
                        replay_status = "error".to_string();
                        replay_error = format!("replay live call failed: {}", e.message());
                        (cp_response.clone(), *checkpoint_duration_ms, true)
                    }
                }
            } else {
                (cp_response.clone(), *checkpoint_duration_ms, true)
            };

            let source = if used_recorded { "recorded" } else { "live" };
            let validated = validate && boundary == "http" && !used_recorded && response_to_store == *cp_response;

            sqlx::query(
                "INSERT INTO flux.checkpoints
                 (execution_id, call_index, boundary, url, method, request, response, duration_ms)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(replay_execution_id)
            .bind(*call_index)
            .bind(boundary.clone())
            .bind(step_url.clone())
            .bind(cp_method.clone())
            .bind(cp_request.clone())
            .bind(response_to_store.clone())
            .bind(step_duration)
            .execute(&mut *tx)
            .await
            .map_err(|e| Status::internal(format!("failed to persist replay checkpoint: {e}")))?;

            replay_output = response_to_store
                .get("body")
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            steps.push(pb::ReplayStep {
                call_index: *call_index,
                boundary: boundary.clone(),
                url: step_url.clone(),
                used_recorded,
                duration_ms: step_duration,
                source: source.to_string(),
                validated,
            });

            if validate && boundary == "http" && !used_recorded && response_to_store != *cp_response {
                replay_status = "error".to_string();
                divergence = Some(pb::ReplayDivergence {
                    checkpoint_index: *call_index,
                    boundary: boundary.clone(),
                    url: step_url.clone(),
                    expected_json: compact_json(cp_response),
                    actual_json: compact_json(&response_to_store),
                });
                replay_error = format!(
                    "replay validation failed at checkpoint {}: live HTTP response diverged from recorded checkpoint\nrecorded  {}\nlive      {}",
                    call_index,
                    summarize_http_response(cp_response),
                    summarize_http_response(&response_to_store),
                );
                break;
            }
        }

        let replay_duration_ms = replay_started.elapsed().as_millis() as i32;

        // Record the replay execution
        let output_json = serde_json::to_string(&replay_output)
            .unwrap_or_else(|_| "null".to_string());

        sqlx::query(
            "INSERT INTO flux.executions
             (id, request_id, method, path, status, request, response, error, code_sha, duration_ms)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NULLIF($8, ''), $9, $10)",
        )
        .bind(replay_execution_id)
        .bind(replay_request_id)
        .bind(method)
        .bind(path)
        .bind(replay_status.clone())
        .bind(request_json)
        .bind(replay_output)
        .bind(replay_error.clone())
        .bind(code_sha)
        .bind(replay_duration_ms)
        .execute(&mut *tx)
        .await
        .map_err(|e| Status::internal(format!("failed to insert replay execution: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Status::internal(format!("failed to commit replay execution: {e}")))?;

        Ok(Response::new(pb::ReplayResponse {
            execution_id: replay_execution_id.to_string(),
            status: replay_status,
            output: output_json,
            error: replay_error,
            duration_ms: replay_duration_ms,
            steps,
            divergence,
        }))
    }

    async fn resume(
        &self,
        request: Request<pb::ResumeRequest>,
    ) -> Result<Response<pb::ResumeResponse>, Status> {
        let _auth_mode = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();

        let source_execution_id = uuid::Uuid::parse_str(&req.execution_id)
            .map_err(|_| Status::invalid_argument("invalid execution_id"))?;

        let source_execution: Option<(
            String,
            String,
            serde_json::Value,
            String,
        )> = sqlx::query_as(
            "SELECT method, path, request, code_sha
             FROM flux.executions
             WHERE id = $1",
        )
        .bind(source_execution_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch source execution: {e}")))?;

        let (method, path, source_request_json, code_sha) =
            source_execution.ok_or_else(|| Status::not_found("execution not found"))?;

        let checkpoint_rows: Vec<(
            i32,
            String,
            Option<String>,
            Option<String>,
            serde_json::Value,
            serde_json::Value,
            i32,
        )> = sqlx::query_as(
            "SELECT call_index, boundary, url, method, request, response, duration_ms
             FROM flux.checkpoints
             WHERE execution_id = $1
             ORDER BY call_index ASC",
        )
        .bind(source_execution_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch checkpoints: {e}")))?;

        if checkpoint_rows.is_empty() {
            return Err(Status::failed_precondition("resume requires at least one checkpoint"));
        }

        let inferred_from_index = checkpoint_rows
            .iter()
            .map(|(call_index, _, _, _, _, _, _)| *call_index)
            .max()
            .unwrap_or(0);
        let from_index = if req.from_index < 0 {
            inferred_from_index
        } else {
            req.from_index.max(0)
        };

        let resume_execution_id = uuid::Uuid::new_v4();
        let resume_request_id = uuid::Uuid::new_v4();

        let mut steps = Vec::with_capacity(checkpoint_rows.len());
        let mut total_duration_ms = 0i32;
        let mut final_status = "ok".to_string();
        let mut final_error = String::new();
        let mut final_output = serde_json::Value::Null;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Status::internal(format!("failed to begin resume transaction: {e}")))?;

        for (call_index, boundary, url, cp_method, request_json, response_json, _duration_ms) in checkpoint_rows {
            let step_url = url.unwrap_or_else(|| {
                request_json
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string()
            });

            let step_method = cp_method.or_else(|| {
                request_json
                    .get("method")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string())
            });

            if call_index < from_index || boundary != "http" {
                sqlx::query(
                    "INSERT INTO flux.checkpoints
                     (execution_id, call_index, boundary, url, method, request, response, duration_ms)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
                )
                .bind(resume_execution_id)
                .bind(call_index)
                .bind(boundary.clone())
                .bind(step_url.clone())
                .bind(step_method)
                .bind(request_json)
                .bind(response_json)
                .bind(0i32)
                .execute(&mut *tx)
                .await
                .map_err(|e| Status::internal(format!("failed to persist recorded resume checkpoint: {e}")))?;

                steps.push(pb::ReplayStep {
                    call_index,
                    boundary,
                    url: step_url,
                    used_recorded: true,
                    duration_ms: 0,
                    source: "recorded".to_string(),
                    validated: false,
                });

                continue;
            }

            let (live_response, duration_ms) = perform_live_http_call(&request_json).await?;
            total_duration_ms += duration_ms;

            sqlx::query(
                "INSERT INTO flux.checkpoints
                 (execution_id, call_index, boundary, url, method, request, response, duration_ms)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
            )
            .bind(resume_execution_id)
            .bind(call_index)
            .bind(boundary.clone())
            .bind(step_url.clone())
            .bind(step_method)
            .bind(request_json)
            .bind(live_response.clone())
            .bind(duration_ms)
            .execute(&mut *tx)
            .await
            .map_err(|e| Status::internal(format!("failed to persist live resume checkpoint: {e}")))?;

            steps.push(pb::ReplayStep {
                call_index,
                boundary,
                url: step_url,
                used_recorded: false,
                duration_ms,
                source: "live".to_string(),
                validated: false,
            });

            final_output = live_response
                .get("body")
                .cloned()
                .unwrap_or(serde_json::Value::Null);

            let status_code = live_response
                .get("status")
                .and_then(|v| v.as_i64())
                .unwrap_or(0);
            if status_code >= 400 || status_code == 0 {
                final_status = "error".to_string();
                final_error = format!("external service returned {}", status_code);
                break;
            }
        }

        let output_json = serde_json::to_string(&final_output).unwrap_or_else(|_| "null".to_string());

        sqlx::query(
            "INSERT INTO flux.executions
             (id, request_id, method, path, status, request, response, error, code_sha, duration_ms)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NULLIF($8, ''), $9, $10)",
        )
        .bind(resume_execution_id)
        .bind(resume_request_id)
        .bind(method)
        .bind(path)
        .bind(final_status.clone())
        .bind(source_request_json)
        .bind(final_output)
        .bind(final_error.clone())
        .bind(code_sha)
        .bind(total_duration_ms)
        .execute(&mut *tx)
        .await
        .map_err(|e| Status::internal(format!("failed to persist resume execution: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Status::internal(format!("failed to commit resume execution: {e}")))?;

        Ok(Response::new(pb::ResumeResponse {
            execution_id: resume_execution_id.to_string(),
            status: final_status,
            output: output_json,
            error: final_error,
            duration_ms: total_duration_ms,
            from_index,
            steps,
        }))
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
