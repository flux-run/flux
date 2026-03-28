use std::collections::HashMap;
use std::net::SocketAddr;

use reqwest::Client;
use reqwest::Url;
use sha2::{Digest, Sha256};
use sqlx::{postgres::PgListener, PgPool};
use tokio::sync::mpsc;
use tokio::sync::watch;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub use shared::pb;

#[derive(Debug, Clone)]
pub struct TenantIdentity {
    pub org_id: String,
    pub project_id: String,
    pub token_id: Option<uuid::Uuid>,
}

fn is_generic_error_label(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "" | "unhandled exception"
            | "unknown runtime error"
            | "unknown error"
            | "runtime error"
            | "exception"
            | "error"
    )
}

fn stack_error_headline(stack: &str) -> Option<String> {
    stack
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with("at "))
        .map(|line| line.strip_prefix("Uncaught ").unwrap_or(line).trim())
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
}

fn stack_error_name_and_message(stack: &str) -> (Option<String>, Option<String>) {
    let Some(headline) = stack_error_headline(stack) else {
        return (None, None);
    };

    if let Some((name, message)) = headline.split_once(':') {
        let normalized_name = name.trim();
        let normalized_message = message.trim();
        if !normalized_name.is_empty()
            && !normalized_message.is_empty()
            && (normalized_name.ends_with("Error")
                || normalized_name.ends_with("Exception")
                || normalized_name == "DOMException")
        {
            return (
                Some(normalized_name.to_string()),
                Some(normalized_message.to_string()),
            );
        }
    }

    (None, Some(headline))
}

fn normalized_issue_title(
    error_name: &str,
    error_message: &str,
    error_stack: &str,
    fallback_error: &str,
) -> String {
    let normalized_name = error_name.trim();
    let normalized_message = error_message.trim();
    let stack_headline = stack_error_headline(error_stack);
    let fallback = fallback_error.trim();

    if !normalized_name.is_empty() && !normalized_message.is_empty() {
        let prefix = format!("{normalized_name}:");
        if normalized_message == normalized_name || normalized_message.starts_with(&prefix) {
            return normalized_message.to_string();
        }
        return format!("{normalized_name}: {normalized_message}");
    }

    if let Some(headline) = stack_headline
        .as_deref()
        .filter(|value| !is_generic_error_label(value))
    {
        return headline.to_string();
    }

    if !normalized_message.is_empty() && !is_generic_error_label(normalized_message) {
        return normalized_message.to_string();
    }

    if !fallback.is_empty() && !is_generic_error_label(fallback) {
        return fallback.to_string();
    }

    if let Some(headline) = stack_headline {
        return headline;
    }

    if !normalized_message.is_empty() {
        return normalized_message.to_string();
    }

    if !fallback.is_empty() {
        return fallback.to_string();
    }

    "Unhandled exception".to_string()
}

fn first_stack_frame(stack: &str) -> String {
    stack
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("at ") || line.contains("://"))
        .unwrap_or_default()
        .to_string()
}

fn issue_fingerprint(
    explicit_fingerprint: &str,
    error_name: &str,
    error_message: &str,
    error_stack: &str,
    fallback_error: &str,
) -> String {
    if !explicit_fingerprint.trim().is_empty() {
        return explicit_fingerprint.trim().to_string();
    }

    let basis = format!(
        "{}|{}|{}",
        error_name.trim(),
        if error_message.trim().is_empty() {
            fallback_error.trim()
        } else {
            error_message.trim()
        },
        first_stack_frame(error_stack),
    );

    hex::encode(Sha256::digest(basis.as_bytes()))
}

fn has_error_details(error_name: &str, error_message: &str, fallback_error: &str) -> bool {
    !error_name.trim().is_empty()
        || !error_message.trim().is_empty()
        || !fallback_error.trim().is_empty()
}

fn normalized_project_id(identity_project_id: &str, request_project_id: &str) -> Option<String> {
    let identity = identity_project_id.trim();
    if !identity.is_empty() && identity != "default" {
        return Some(identity.to_string());
    }

    let request = request_project_id.trim();
    if !request.is_empty() && request != "default" {
        return Some(request.to_string());
    }

    None
}

fn execution_phase_label(phase: &str) -> &'static str {
    match phase.trim() {
        "init" => "Initialization",
        "external" => "External dependency",
        "runtime" => "Runtime execution",
        _ => "Runtime execution",
    }
}

#[derive(Clone)]
pub struct InternalAuthGrpc {
    pool: PgPool,
    #[allow(dead_code)]
    expected_token: String,
    mode: String,
    cache: moka::future::Cache<String, TenantIdentity>,
}

impl InternalAuthGrpc {
    pub fn new(pool: PgPool, expected_token: String) -> Self {
        let mode = std::env::var("FLUX_MODE").unwrap_or_else(|_| "standalone".to_string());
        let cache = moka::future::Cache::builder()
            .max_capacity(10_000)
            .time_to_live(std::time::Duration::from_secs(60))
            .build();

        Self {
            pool,
            expected_token,
            mode,
            cache,
        }
    }

    async fn resolve_db_token(&self, token: &str) -> Result<Option<TenantIdentity>, sqlx::Error> {
        let token_hash = hex::encode(Sha256::digest(token.as_bytes()));

        let row: Option<(uuid::Uuid, uuid::Uuid, uuid::Uuid)> = sqlx::query_as(
            "SELECT id, org_id, project_id FROM control.service_tokens \
             WHERE token_hash = $1 AND revoked = false",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|(token_id, org_id, project_id)| TenantIdentity {
            org_id: org_id.to_string(),
            project_id: project_id.to_string(),
            token_id: Some(token_id),
        }))
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
    ) -> Result<TenantIdentity, Status> {
        if self.mode == "standalone" {
            return Ok(TenantIdentity {
                org_id: "default".to_string(),
                project_id: "default".to_string(),
                token_id: None,
            });
        }

        let provided_token = Self::read_bearer_token(metadata).unwrap_or_default();
        if provided_token.is_empty() {
            return Err(Status::unauthenticated(
                "missing authorization bearer token",
            ));
        }

        if !provided_token.starts_with("flux_sk_") {
            return Err(Status::unauthenticated("invalid token prefix"));
        }

        // Check cache
        let token_hash = hex::encode(Sha256::digest(provided_token.as_bytes()));
        if let Some(identity) = self.cache.get(&token_hash).await {
            return Ok(identity);
        }

        // DB lookup
        let identity = self
            .resolve_db_token(&provided_token)
            .await
            .map_err(|e| Status::internal(format!("token lookup failed: {e}")))?
            .ok_or_else(|| Status::unauthenticated("invalid service token"))?;

        // Update last_used_at in background
        let pool = self.pool.clone();
        let hash_copy = token_hash.clone();
        tokio::spawn(async move {
            let _ = sqlx::query(
                "UPDATE control.service_tokens SET last_used_at = now() WHERE token_hash = $1",
            )
            .bind(hash_copy)
            .execute(&pool)
            .await;
        });

        // Populate cache
        self.cache.insert(token_hash, identity.clone()).await;

        Ok(identity)
    }

    async fn resolve_execution_id(&self, raw: &str) -> Result<uuid::Uuid, Status> {
        if let Ok(id) = uuid::Uuid::parse_str(raw) {
            return Ok(id);
        }

        if raw.len() < 8 {
            return Err(Status::invalid_argument(
                "execution_id must be a full UUID or an 8+ character prefix",
            ));
        }

        let matches: Vec<uuid::Uuid> =
            sqlx::query_scalar("SELECT id FROM flux.executions WHERE id::text LIKE $1")
                .bind(format!("{}%", raw))
                .fetch_all(&self.pool)
                .await
                .map_err(|e| Status::internal(format!("failed to resolve execution_id: {e}")))?;

        if matches.is_empty() {
            return Err(Status::not_found(format!(
                "no execution found matching '{raw}'"
            )));
        }

        if matches.len() > 1 {
            return Err(Status::invalid_argument(format!(
                "multiple executions match prefix '{raw}', please provide more characters"
            )));
        }

        Ok(matches[0])
    }
}

#[derive(Debug, Clone)]
struct WhyExecution {
    status: String,
    duration_ms: i32,
    error: Option<String>,
    error_name: Option<String>,
    error_message: Option<String>,
    error_phase: Option<String>,
    error_source: Option<String>,
    is_user_code: Option<bool>,
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
    let fallback_error = exec.error.as_deref().unwrap_or_default();
    let error_name = exec.error_name.as_deref().unwrap_or_default();
    let error_message = exec.error_message.as_deref().unwrap_or_default();
    let error_headline = normalized_issue_title(error_name, error_message, "", fallback_error);
    let has_error = has_error_details(error_name, error_message, fallback_error);
    let error_phase = execution_phase_label(exec.error_phase.as_deref().unwrap_or("runtime"));
    let is_platform_failure = exec.is_user_code == Some(false)
        || matches!(
            exec.error_source.as_deref(),
            Some("platform_runtime" | "platform_executor")
        );

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
                    let mut reason = format!(
                        "external service error\ncall    {} {}\nstatus  {}\nindex   {}",
                        last.boundary.to_uppercase(),
                        url,
                        status,
                        last.call_index
                    );
                    if has_error {
                        reason.push_str(&format!("\nerror   {}", error_headline));
                    }
                    return (
                        reason,
                        "the upstream service returned a 5xx — not a bug in your code".to_string(),
                    );
                }

                if status == 429 {
                    let mut reason = format!("rate limited\ncall    {}", url);
                    if has_error {
                        reason.push_str(&format!("\nerror   {}", error_headline));
                    }
                    return (reason, "add retry with exponential backoff".to_string());
                }

                if status == 401 || status == 403 {
                    let mut reason = format!("auth failure\ncall    {}\nstatus  {}", url, status);
                    if has_error {
                        reason.push_str(&format!("\nerror   {}", error_headline));
                    }
                    return (
                        reason,
                        "check credentials/token for this service".to_string(),
                    );
                }

                if status == 0 {
                    let mut reason =
                        format!("network failure — no response received\ncall    {}", url);
                    if has_error {
                        reason.push_str(&format!("\nerror   {}", error_headline));
                    }
                    return (
                        reason,
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
                let error_detail = if error.trim().is_empty() {
                    error_headline.as_str()
                } else {
                    error
                };

                if error.contains("duplicate key") || error.contains("unique") {
                    return (
                        format!(
                            "duplicate key violation\nquery   {}\nerror   {}",
                            query, error_detail
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
                    format!(
                        "database error\nquery   {}\nerror   {}",
                        query, error_detail
                    ),
                    String::new(),
                );
            }
        }

        let summary = if has_error {
            error_headline
        } else {
            "Unhandled exception".to_string()
        };

        if is_platform_failure {
            let suggestion = if exec.error_phase.as_deref() == Some("init") {
                "inspect runtime/bootstrap initialization and retry the execution".to_string()
            } else {
                "retry once and inspect platform/runtime health if it persists".to_string()
            };

            return (
                format!(
                    "runtime failure before response generation\nerror   {}\nphase   {}",
                    summary, error_phase
                ),
                suggestion,
            );
        }

        return (
            format!(
                "unhandled exception in user code\nerror   {}\nphase   {}",
                summary, error_phase
            ),
            "remove or handle the thrown exception before the request returns".to_string(),
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

async fn perform_live_http_call(
    request: &serde_json::Value,
) -> Result<(serde_json::Value, i32), Status> {
    let url = request
        .get("url")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if url.is_empty() {
        return Err(Status::invalid_argument(
            "resume: checkpoint request missing url",
        ));
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

    let body = value
        .get("body")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    let body_text = serde_json::to_string(&body).unwrap_or_else(|_| "null".to_string());
    let shortened_body = if body_text.len() > 160 {
        format!("{}...", &body_text[..160])
    } else {
        body_text
    };

    format!("status={} body={}", status, shortened_body)
}

async fn perform_runtime_resume(
    request_json: &serde_json::Value,
    recorded_checkpoints: Vec<serde_json::Value>,
    token: &str,
) -> Result<serde_json::Value, Status> {
    let original_url = request_json
        .get("url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            Status::failed_precondition("resume currently requires an HTTP listener execution")
        })?;

    let mut resume_url = Url::parse(original_url)
        .map_err(|error| Status::internal(format!("failed to parse runtime url: {error}")))?;
    resume_url.set_path("/__flux_internal/resume");
    resume_url.set_query(None);

    let response = Client::new()
        .post(resume_url)
        .header("x-internal-token", token)
        .json(&serde_json::json!({
            "request": request_json,
            "recorded_checkpoints": recorded_checkpoints,
        }))
        .send()
        .await
        .map_err(|error| Status::internal(format!("resume runtime call failed: {error}")))?;

    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "failed to read runtime error response".to_string());
        return Err(Status::internal(format!(
            "resume runtime rejected request: {} {}",
            status, body
        )));
    }

    response.json::<serde_json::Value>().await.map_err(|error| {
        Status::internal(format!("failed to decode runtime resume response: {error}"))
    })
}

fn compact_json(value: &serde_json::Value) -> String {
    let rendered = serde_json::to_string(value).unwrap_or_else(|_| "null".to_string());
    if rendered.len() > 240 {
        format!("{}...", &rendered[..240])
    } else {
        rendered
    }
}

fn compact_json_or_missing(value: Option<&serde_json::Value>) -> String {
    match value {
        Some(value) => compact_json(value),
        None => "null".to_string(),
    }
}

fn push_json_diffs(
    path: &str,
    expected: Option<&serde_json::Value>,
    actual: Option<&serde_json::Value>,
    out: &mut Vec<pb::ReplayFieldDiff>,
) {
    if expected == actual {
        return;
    }

    match (expected, actual) {
        (Some(serde_json::Value::Object(left)), Some(serde_json::Value::Object(right))) => {
            let mut keys = std::collections::BTreeSet::new();
            keys.extend(left.keys().cloned());
            keys.extend(right.keys().cloned());

            for key in keys {
                let child_path = if path == "$" {
                    format!("$.{}", key)
                } else {
                    format!("{}.{}", path, key)
                };
                let left_value = left.get(&key);
                let right_value = right.get(&key);
                push_json_diffs(&child_path, left_value, right_value, out);
            }
        }
        (Some(serde_json::Value::Array(left)), Some(serde_json::Value::Array(right))) => {
            let max_len = left.len().max(right.len());
            for index in 0..max_len {
                let child_path = format!("{}[{}]", path, index);
                let left_value = left.get(index);
                let right_value = right.get(index);
                push_json_diffs(&child_path, left_value, right_value, out);
            }
        }
        (None, Some(_)) => {
            out.push(pb::ReplayFieldDiff {
                path: path.to_string(),
                expected_json: compact_json_or_missing(expected),
                actual_json: compact_json_or_missing(actual),
                kind: "added".to_string(),
            });
        }
        (Some(_), None) => {
            out.push(pb::ReplayFieldDiff {
                path: path.to_string(),
                expected_json: compact_json_or_missing(expected),
                actual_json: compact_json_or_missing(actual),
                kind: "removed".to_string(),
            });
        }
        _ => {
            out.push(pb::ReplayFieldDiff {
                path: path.to_string(),
                expected_json: compact_json_or_missing(expected),
                actual_json: compact_json_or_missing(actual),
                kind: "changed".to_string(),
            });
        }
    }
}

fn diff_json_values(
    expected: &serde_json::Value,
    actual: &serde_json::Value,
) -> Vec<pb::ReplayFieldDiff> {
    let mut diffs = Vec::new();
    push_json_diffs("$", Some(expected), Some(actual), &mut diffs);
    diffs
}

#[tonic::async_trait]
impl pb::internal_auth_service_server::InternalAuthService for InternalAuthGrpc {
    type TailStream = ReceiverStream<Result<pb::TailEvent, Status>>;

    async fn validate_token(
        &self,
        request: Request<pb::ValidateTokenRequest>,
    ) -> Result<Response<pb::ValidateTokenResponse>, Status> {
        let identity = self.authenticate(request.metadata()).await?;

        Ok(Response::new(pb::ValidateTokenResponse {
            ok: true,
            auth_mode: format!("{}:{}", identity.org_id, identity.project_id),
            project_id: identity.project_id,
        }))
    }

    async fn list_logs(
        &self,
        request: Request<pb::ListLogsRequest>,
    ) -> Result<Response<pb::ListLogsResponse>, Status> {
        let identity = self.authenticate(request.metadata()).await?;
        let limit = request.into_inner().limit.max(1).min(500) as i64;

        let rows: Vec<(
            String,
            String,
            Option<String>,
            String,
            String,
            String,
            i32,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
        )> = sqlx::query_as(
            "SELECT \
                id::text, \
                request_id::text, \
                project_id, \
                method, \
                path, \
                status, \
                duration_ms, \
                code_sha, \
                error, \
                error_source, \
                error_type, \
                to_char(started_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') \
             FROM flux.executions \
             WHERE org_id = $1 \
             ORDER BY started_at DESC \
             LIMIT $2",
        )
        .bind(identity.org_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to list logs: {e}")))?;

        let logs = rows
            .into_iter()
            .map(
                |(
                    execution_id,
                    request_id,
                    _project_id,
                    method,
                    path,
                    status,
                    duration_ms,
                    code_version,
                    error,
                    error_source,
                    error_type,
                    timestamp,
                )| pb::LogEntry {
                    execution_id,
                    request_id,
                    method,
                    path,
                    status,
                    duration_ms,
                    timestamp: timestamp.unwrap_or_default(),
                    error: error.unwrap_or_default(),
                    code_version: code_version.unwrap_or_default(),
                    project_id: _project_id.unwrap_or_default(),
                    error_source: error_source.unwrap_or_default(),
                    error_type: error_type.unwrap_or_default(),
                },
            )
            .collect();

        Ok(Response::new(pb::ListLogsResponse { logs }))
    }

    async fn record_execution(
        &self,
        request: Request<pb::RecordExecutionRequest>,
    ) -> Result<Response<pb::RecordExecutionResponse>, Status> {
        let identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        tracing::info!(
            execution_id = %req.execution_id,
            request_id = %req.request_id,
            org_id = %identity.org_id,
            project_id = %identity.project_id,
            request_project_id = %req.project_id,
            method = %req.method,
            path = %req.path,
            status = %req.status,
            "record_execution called"
        );

        let execution_id = uuid::Uuid::parse_str(&req.execution_id)
            .map_err(|e| Status::invalid_argument(format!("invalid execution_id: {e}")))?;
        let request_id = uuid::Uuid::parse_str(&req.request_id)
            .map_err(|e| Status::invalid_argument(format!("invalid request_id: {e}")))?;

        let request_json: serde_json::Value = serde_json::from_str(&req.request_json)
            .map_err(|e| Status::invalid_argument(format!("invalid request_json: {e}")))?;
        let response_json: serde_json::Value = serde_json::from_str(&req.response_json)
            .map_err(|e| Status::invalid_argument(format!("invalid response_json: {e}")))?;

        let project_id = normalized_project_id(&identity.project_id, &req.project_id);
        let org_id = identity.org_id;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Status::internal(format!("failed to begin transaction: {e}")))?;

        let request_headers_json: serde_json::Value =
            serde_json::from_str(&req.request_headers_json).unwrap_or(serde_json::Value::Null);
        let (stack_error_name, stack_error_message) =
            stack_error_name_and_message(&req.error_stack);
        let error_name = (!req.error_name.trim().is_empty())
            .then_some(req.error_name.clone())
            .or(stack_error_name.clone());
        let error_message = (!req.error_message.trim().is_empty())
            .then_some(req.error_message.clone())
            .or(stack_error_message.clone())
            .or((!req.error.trim().is_empty()).then_some(req.error.clone()));
        let error_phase = (!req.error_phase.trim().is_empty()).then_some(req.error_phase.clone());
        let is_user_code = if req.status == "ok" || req.status == "running" {
            None
        } else {
            Some(req.is_user_code)
        };

        // Parse structured frames sent by the runtime.
        let error_frames: serde_json::Value =
            serde_json::from_str(&req.error_frames_json).unwrap_or(serde_json::Value::Null);
        let first_frame = error_frames.as_array().and_then(|arr| arr.first());
        let failure_point_file = first_frame
            .and_then(|f| f.get("file"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned);
        let failure_point_line = first_frame
            .and_then(|f| f.get("line"))
            .and_then(|v| v.as_i64())
            .map(|n| n as i32);
        let aborted = (req.status == "error" || req.status == "critical").then_some(true);
        let response_sent = (req.response_status > 0).then_some(true);
        let error_frames_json = if error_frames.is_null() { None } else { Some(error_frames.clone()) };

        // Resolve function_id from route — used in the execution row and issue upsert.
        let route_project_id = project_id
            .as_deref()
            .and_then(|v| uuid::Uuid::parse_str(v).ok());
        let function_id: Option<uuid::Uuid> = if let Some(rpid) = route_project_id {
            let row: Option<(uuid::Uuid,)> = sqlx::query_as(
                "SELECT function_id FROM control.routes WHERE project_id = $1 AND method = $2 AND path = $3",
            )
            .bind(rpid)
            .bind(if req.request_method.trim().is_empty() { req.method.clone() } else { req.request_method.clone() })
            .bind(req.path.clone())
            .fetch_optional(&mut *tx)
            .await
            .map_err(|e| Status::internal(format!("failed to resolve route: {e}")))?;
            row.map(|(fid,)| fid)
        } else {
            None
        };

        sqlx::query(
            "INSERT INTO flux.executions \
             (id, request_id, project_id, org_id, method, path, status, request, response, error, code_sha, duration_ms, token_id, \
              client_ip, user_agent, request_method, request_headers, request_body, response_status, response_body, error_name, error_message, error_stack, error_fingerprint, error_phase, is_user_code, error_source, error_type, function_id) \
             VALUES ($1, $2, $3, $4, $5, $6, 'running', $7, NULL, NULL, $8, 0, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18, $19, $20, $21, $22, $23, $24, $25) \
             ON CONFLICT (id) DO NOTHING",
        )
        .bind(execution_id)
        .bind(request_id)
        .bind(project_id.clone())
        .bind(org_id.clone())
        .bind(req.method.clone())
        .bind(req.path.clone())
        .bind(request_json.clone())
        .bind(req.code_version.clone())
        .bind(identity.token_id)
        .bind(req.client_ip.clone())
        .bind(req.user_agent.clone())
        .bind(req.request_method.clone())
        .bind(request_headers_json.clone())
        .bind(req.request_body.clone())
        .bind(req.response_status)
        .bind(req.response_body.clone())
        .bind(error_name.clone())
        .bind(error_message.clone())
        .bind(req.error_stack.clone())
        .bind(req.error_fingerprint.clone())
        .bind(error_phase.clone())
        .bind(is_user_code)
        .bind(req.error_source.clone())
        .bind(req.error_type.clone())
        .bind(function_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| Status::internal(format!("failed to insert execution: {e}")))?;

        sqlx::query(
            "UPDATE flux.executions \
             SET method = $2, path = $3, status = $4, request = $5, response = $6, \
                 error = NULLIF($7, ''), code_sha = $8, duration_ms = $9, project_id = $10, org_id = $11, \
                 client_ip = $12, user_agent = $13, request_method = $14, request_headers = $15, request_body = $16, \
                 response_status = $17, response_body = $18, error_name = $19, error_message = $20, error_stack = $21, error_fingerprint = $22, \
                 error_phase = $23, is_user_code = $24, error_source = $25, error_type = $26, \
                 function_id = COALESCE($27, function_id), failure_point_file = $28, failure_point_line = $29, \
                 aborted = $30, response_sent = $31, error_frames = $32 \
             WHERE id = $1",
        )
        .bind(execution_id)
        .bind(req.method.clone())
        .bind(req.path.clone())
        .bind(req.status.clone())
        .bind(request_json)
        .bind(response_json)
        .bind(req.error.clone())
        .bind(req.code_version.clone())
        .bind(req.duration_ms)
        .bind(project_id.clone())
        .bind(org_id.clone())
        .bind(req.client_ip.clone())
        .bind(req.user_agent.clone())
        .bind(req.request_method.clone())
        .bind(request_headers_json)
        .bind(req.request_body.clone())
        .bind(req.response_status)
        .bind(req.response_body.clone())
        .bind(error_name.clone())
        .bind(error_message.clone())
        .bind(req.error_stack.clone())
        .bind(req.error_fingerprint.clone())
        .bind(error_phase.clone())
        .bind(is_user_code)
        .bind(req.error_source.clone())
        .bind(req.error_type.clone())
        .bind(function_id)
        .bind(failure_point_file)
        .bind(failure_point_line)
        .bind(aborted)
        .bind(response_sent)
        .bind(error_frames_json)
        .execute(&mut *tx)
        .await
        .map_err(|e| Status::internal(format!("failed to update execution: {e}")))?;

        // Automated Issue Grouping / Fingerprinting
        if req.status == "error" || req.status == "critical" {
            let error_msg = req.error.clone();
            let error_stack = req.error_stack.clone();
            let fingerprint = issue_fingerprint(
                &req.error_fingerprint,
                error_name.as_deref().unwrap_or_default(),
                error_message.as_deref().unwrap_or_default(),
                &error_stack,
                &error_msg,
            );
            let title = normalized_issue_title(
                error_name.as_deref().unwrap_or_default(),
                error_message.as_deref().unwrap_or_default(),
                &error_stack,
                &error_msg,
            );
            let sample_message = if let Some(message) = error_message.as_deref() {
                message.to_string()
            } else if req.error_message.trim().is_empty() {
                if title.trim().is_empty() {
                    req.error.clone()
                } else {
                    title.clone()
                }
            } else {
                req.error_message.clone()
            };

            // Reuse the function_id already resolved at the top of this transaction.
            if let Some(fid) = function_id {
                sqlx::query(
                    "INSERT INTO flux.issues \
                     (function_id, fingerprint, title, sample_execution_id, sample_stack, sample_message, occurrence_count, last_seen) \
                     VALUES ($1, $2, $3, $4, $5, $6, 1, NOW()) \
                     ON CONFLICT (function_id, fingerprint) DO UPDATE SET \
                       occurrence_count = flux.issues.occurrence_count + 1, \
                       last_seen = NOW(), \
                       sample_execution_id = EXCLUDED.sample_execution_id, \
                       sample_stack = EXCLUDED.sample_stack, \
                       sample_message = EXCLUDED.sample_message"
                )
                .bind(fid)
                .bind(fingerprint)
                .bind(title)
                .bind(execution_id)
                .bind(req.error_stack)
                .bind(sample_message)
                .execute(&mut *tx)
                .await
                .map_err(|e| Status::internal(format!("failed to upsert issue: {e}")))?;
            }
        }

        for checkpoint in req.checkpoints {
            let request_json: serde_json::Value =
                serde_json::from_str(&checkpoint.request_json).unwrap_or(serde_json::Value::Null);
            let response_json: serde_json::Value =
                serde_json::from_str(&checkpoint.response_json).unwrap_or(serde_json::Value::Null);

            sqlx::query(
                "INSERT INTO flux.checkpoints \
                 (execution_id, call_index, org_id, boundary, url, method, request, response, duration_ms) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) \
                 ON CONFLICT (execution_id, call_index) DO UPDATE SET \
                   response = EXCLUDED.response, \
                   duration_ms = EXCLUDED.duration_ms",
            )
            .bind(execution_id)
            .bind(checkpoint.call_index as i32)
            .bind(org_id.clone())
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

        for log in req.logs.into_iter() {
            sqlx::query(
                "INSERT INTO flux.execution_console_logs \
                 (execution_id, seq, org_id, level, message) \
                 VALUES ($1, $2, $3, $4, $5) \
                 ON CONFLICT (execution_id, seq) DO UPDATE SET \
                   level = EXCLUDED.level, \
                   message = EXCLUDED.message",
            )
            .bind(execution_id)
            .bind(log.seq as i32)
            .bind(org_id.clone())
            .bind(log.level)
            .bind(log.message)
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                Status::internal(format!("failed to upsert execution console log: {e}"))
            })?;
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
        let identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let execution_id_raw = req.execution_id.clone();
        let execution_id = self.resolve_execution_id(&execution_id_raw).await?;

        let execution: Option<(
            String,
            String,
            String,
            i32,
            Option<String>,
            serde_json::Value,
            serde_json::Value,
        )> = sqlx::query_as(
            "SELECT method, path, status, duration_ms, error, request, response \
             FROM flux.executions \
             WHERE id = $1 AND org_id = $2",
        )
        .bind(execution_id)
        .bind(identity.org_id.clone())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch execution: {e}")))?;

        let (method, path, status, duration_ms, error, request_json, response_json) =
            execution.ok_or_else(|| Status::not_found("execution not found"))?;

        let checkpoint_rows: Vec<(i32, String, serde_json::Value, serde_json::Value, i32)> =
            sqlx::query_as(
                "SELECT call_index, boundary, request, response, duration_ms \
                 FROM flux.checkpoints \
                 WHERE execution_id = $1 AND org_id = $2 \
                 ORDER BY call_index ASC",
            )
            .bind(execution_id)
            .bind(identity.org_id.clone())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("failed to fetch checkpoints: {e}")))?;

        let checkpoints = checkpoint_rows
            .into_iter()
            .map(
                |(call_index, boundary, request, response, duration_ms)| pb::Checkpoint {
                    call_index,
                    boundary,
                    request: serde_json::to_vec(&request).unwrap_or_default(),
                    response: serde_json::to_vec(&response).unwrap_or_default(),
                    duration_ms,
                },
            )
            .collect();

        let console_log_rows: Vec<(i32, String, String)> = sqlx::query_as(
            "SELECT seq, level, message \
             FROM flux.execution_console_logs \
             WHERE execution_id = $1 AND org_id = $2 \
             ORDER BY seq ASC",
        )
        .bind(execution_id)
        .bind(identity.org_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch execution console logs: {e}")))?;

        let logs = console_log_rows
            .into_iter()
            .map(|(seq, level, message)| pb::ConsoleLogEntry { level, message, seq: seq as u32 })
            .collect();

        Ok(Response::new(pb::GetTraceResponse {
            execution_id: execution_id_raw,
            method,
            path,
            status,
            duration_ms,
            error: error.unwrap_or_default(),
            checkpoints,
            request_json: serde_json::to_string(&request_json)
                .unwrap_or_else(|_| "null".to_string()),
            response_json: serde_json::to_string(&response_json)
                .unwrap_or_else(|_| "null".to_string()),
            logs,
        }))
    }

    async fn tail(
        &self,
        request: Request<pb::TailRequest>,
    ) -> Result<Response<Self::TailStream>, Status> {
        let identity = self.authenticate(request.metadata()).await?;
        let pool = self.pool.clone();
        let project_id = request.into_inner().project_id;

        let (tx, rx) = mpsc::channel(32);

        tokio::spawn(async move {
            tracing::info!(project_id = %project_id, "tail: starting listener task");

            loop {
                let mut listener = match PgListener::connect_with(&pool).await {
                    Ok(mut listener) => {
                        if let Err(err) = listener.listen("flux_executions").await {
                            tracing::error!(error = %err, "tail listener subscribe failed, retrying in 2s");
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            continue;
                        }
                        tracing::info!(
                            "tail: listener connected and subscribed to flux_executions"
                        );
                        listener
                    }
                    Err(err) => {
                        tracing::error!(error = %err, "tail listener connect failed, retrying in 2s");
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        continue;
                    }
                };

                loop {
                    match listener.recv().await {
                        Ok(notification) => {
                            let payload = notification.payload();
                            tracing::info!(payload = %payload, "tail: received notification");
                            let Ok(val) = serde_json::from_str::<serde_json::Value>(payload) else {
                                tracing::warn!(payload = %payload, "tail: received invalid json notification");
                                continue;
                            };

                            let identity_org_id = val
                                .get("org_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default();
                            if identity_org_id != identity.org_id {
                                continue;
                            }

                            let entry_project_id = val
                                .get("project_id")
                                .and_then(|v| v.as_str())
                                .unwrap_or_default();
                            if !project_id.is_empty() && entry_project_id != project_id {
                                continue;
                            }

                            let event = pb::TailEvent {
                                execution_id: val
                                    .get("id")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                method: val
                                    .get("method")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                path: val
                                    .get("path")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                status: val
                                    .get("status")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                duration_ms: val
                                    .get("duration_ms")
                                    .and_then(|v| v.as_i64())
                                    .unwrap_or_default()
                                    as i32,
                                error: val
                                    .get("error")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                started_at: 0,
                            };

                            if tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                        Err(err) => {
                            tracing::error!(error = %err, "tail listener connection lost, reconnecting...");
                            break;
                        }
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
        let identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let execution_id_raw = req.execution_id.clone();
        let execution_id = self.resolve_execution_id(&execution_id_raw).await?;

        let execution: Option<(
            String,
            String,
            String,
            i32,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<String>,
            Option<bool>,
            serde_json::Value,
        )> = sqlx::query_as(
            "SELECT status, method, path, duration_ms, error, error_name, error_message, \
                    error_phase, error_source, is_user_code, COALESCE(response, '{}'::jsonb) as response \
             FROM flux.executions \
             WHERE id = $1 AND org_id = $2",
        )
        .bind(execution_id)
        .bind(identity.org_id.clone())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch execution: {e}")))?;

        let (
            status,
            method,
            path,
            duration_ms,
            db_error,
            error_name,
            error_message,
            error_phase,
            error_source,
            is_user_code,
            response_json,
        ) = execution.ok_or_else(|| Status::not_found("execution not found"))?;

        // Extract the real error body from the response JSONB.
        // The runtime encodes opaque bodies as "__FLUX_B64:<base64>".
        let error_body: String = {
            let raw_body = response_json
                .get("net_response")
                .and_then(|v| v.get("body"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if let Some(b64) = raw_body.strip_prefix("__FLUX_B64:") {
                use base64::Engine;
                base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .ok()
                    .and_then(|b| String::from_utf8(b).ok())
                    .unwrap_or_else(|| raw_body.to_string())
            } else if !raw_body.is_empty() {
                raw_body.to_string()
            } else {
                db_error.clone().unwrap_or_default()
            }
        };

        let checkpoint_rows: Vec<(i32, String, serde_json::Value, serde_json::Value, i32)> =
            sqlx::query_as(
                "SELECT call_index, boundary, request, response, duration_ms \
                 FROM flux.checkpoints \
                 WHERE execution_id = $1 AND org_id = $2 \
                 ORDER BY call_index ASC",
            )
            .bind(execution_id)
            .bind(identity.org_id.clone())
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("failed to fetch checkpoints: {e}")))?;

        let checkpoints: Vec<WhyCheckpoint> = checkpoint_rows
            .into_iter()
            .map(
                |(call_index, boundary, request, response, duration_ms)| WhyCheckpoint {
                    call_index,
                    boundary,
                    request,
                    response,
                    duration_ms,
                },
            )
            .collect();

        // Fetch console logs for this execution
        let console_log_rows: Vec<(i32, String, String)> = sqlx::query_as(
            "SELECT seq, level, message \
             FROM flux.execution_console_logs \
             WHERE execution_id = $1 AND org_id = $2 \
             ORDER BY seq ASC",
        )
        .bind(execution_id)
        .bind(identity.org_id.clone())
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let logs = console_log_rows
            .into_iter()
            .map(|(seq, level, message)| pb::ConsoleLogEntry { level, message, seq: seq as u32 })
            .collect::<Vec<_>>();

        let effective_error = if error_body.is_empty() {
            db_error.clone()
        } else {
            Some(error_body.clone())
        };

        let (reason, suggestion) = analyze_execution(
            &WhyExecution {
                status: status.clone(),
                duration_ms,
                error: effective_error,
                error_name,
                error_message,
                error_phase,
                error_source,
                is_user_code,
            },
            &checkpoints,
        );

        Ok(Response::new(pb::WhyResponse {
            execution_id: execution_id_raw,
            reason,
            suggestion,
            error_body,
            logs,
            method,
            path,
            status,
            duration_ms,
        }))
    }

    async fn replay(
        &self,
        request: Request<pb::ReplayRequest>,
    ) -> Result<Response<pb::ReplayResponse>, Status> {
        let identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let source_execution_id_raw = req.execution_id.clone();
        let source_execution_id = self.resolve_execution_id(&source_execution_id_raw).await?;
        let commit = req.commit;
        let validate = req.validate;

        if validate && !commit {
            return Err(Status::invalid_argument(
                "replay validation requires commit mode so live checkpoint results can be compared",
            ));
        }

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
            "SELECT method, path, request, response, status, error, duration_ms, code_sha \
             FROM flux.executions \
             WHERE id = $1 AND org_id = $2",
        )
        .bind(source_execution_id)
        .bind(identity.org_id.clone())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch source execution: {e}")))?;

        let (method, path, request_json, response_json, _status, _error, _duration_ms, code_sha) =
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
            "SELECT call_index, boundary, url, method, request, response, duration_ms \
             FROM flux.checkpoints \
             WHERE execution_id = $1 AND call_index >= $2 AND org_id = $3 \
             ORDER BY call_index ASC",
        )
        .bind(source_execution_id)
        .bind(from_index)
        .bind(identity.org_id.clone())
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
        let mut tx =
            self.pool.begin().await.map_err(|e| {
                Status::internal(format!("failed to begin replay transaction: {e}"))
            })?;

        let mut steps = Vec::with_capacity(checkpoint_rows.len());
        let mut replay_status = "ok".to_string();
        let mut replay_error = String::new();
        let replay_output = response_json.clone().unwrap_or(serde_json::Value::Null);
        let mut divergence: Option<pb::ReplayDivergence> = None;

        for (
            call_index,
            boundary,
            url,
            cp_method,
            cp_request,
            cp_response,
            checkpoint_duration_ms,
        ) in &checkpoint_rows
        {
            let step_url = url.clone().unwrap_or_else(|| {
                cp_request
                    .get("url")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string()
            });

            // If commit is true, re-execute HTTP calls live; otherwise use recorded.
            let (response_to_store, step_duration, used_recorded) = if commit && boundary == "http"
            {
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
            let validated = validate
                && boundary == "http"
                && !used_recorded
                && response_to_store == *cp_response;

            sqlx::query(
                "INSERT INTO flux.checkpoints \
                 (execution_id, call_index, org_id, boundary, url, method, request, response, duration_ms) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(replay_execution_id)
            .bind(*call_index)
            .bind(identity.org_id.clone())
            .bind(boundary.clone())
            .bind(step_url.clone())
            .bind(cp_method.clone())
            .bind(cp_request.clone())
            .bind(response_to_store.clone())
            .bind(step_duration)
            .execute(&mut *tx)
            .await
            .map_err(|e| Status::internal(format!("failed to persist replay checkpoint: {e}")))?;

            steps.push(pb::ReplayStep {
                call_index: *call_index,
                boundary: boundary.clone(),
                url: step_url.clone(),
                used_recorded,
                duration_ms: step_duration,
                source: source.to_string(),
                validated,
            });

            if validate && boundary == "http" && !used_recorded && response_to_store != *cp_response
            {
                replay_status = "error".to_string();
                divergence = Some(pb::ReplayDivergence {
                    checkpoint_index: *call_index,
                    boundary: boundary.clone(),
                    url: step_url.clone(),
                    expected_json: compact_json(cp_response),
                    actual_json: compact_json(&response_to_store),
                    diffs: diff_json_values(cp_response, &response_to_store),
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

        // If no checkpoint-level error was recorded, check the HTTP response status
        // to determine if the replay itself resulted in a 4xx/5xx.
        if replay_status == "ok" {
            let http_status = replay_output
                .get("net_response")
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            if http_status >= 400 {
                replay_status = "error".to_string();
            }
        }

        // Record the replay execution
        let output_json =
            serde_json::to_string(&replay_output).unwrap_or_else(|_| "null".to_string());

        sqlx::query(
            "INSERT INTO flux.executions \
             (id, request_id, org_id, project_id, method, path, status, request, response, error, code_sha, duration_ms, token_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULLIF($10, ''), $11, $12, $13)",
        )
        .bind(replay_execution_id)
        .bind(replay_request_id)
        .bind(identity.org_id.clone())
        .bind(identity.project_id.clone())
        .bind(method)
        .bind(path)
        .bind(replay_status.clone())
        .bind(request_json)
        .bind(replay_output)
        .bind(replay_error.clone())
        .bind(code_sha)
        .bind(replay_duration_ms)
        .bind(identity.token_id)
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
        let auth_token = Self::read_bearer_token(request.metadata())
            .ok_or_else(|| Status::unauthenticated("missing authorization bearer token"))?;
        let identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let source_execution_id_raw = req.execution_id.clone();
        let source_execution_id = self.resolve_execution_id(&source_execution_id_raw).await?;

        let source_execution: Option<(String, String, serde_json::Value, String)> = sqlx::query_as(
            "SELECT method, path, request, code_sha \
             FROM flux.executions \
             WHERE id = $1 AND org_id = $2",
        )
        .bind(source_execution_id)
        .bind(identity.org_id.clone())
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
            "SELECT call_index, boundary, url, method, request, response, duration_ms \
             FROM flux.checkpoints \
             WHERE execution_id = $1 AND org_id = $2 \
             ORDER BY call_index ASC",
        )
        .bind(source_execution_id)
        .bind(identity.org_id.clone())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to fetch checkpoints: {e}")))?;

        if checkpoint_rows.is_empty() {
            // Original execution had no checkpoints (threw before any IO).
            // Fall through as a fully-live execution from index 0 with no
            // recorded checkpoints — equivalent to running the request fresh.
        }

        let inferred_from_index = if checkpoint_rows.is_empty() {
            0
        } else {
            checkpoint_rows
                .iter()
                .map(|(call_index, _, _, _, _, _, _)| *call_index)
                .max()
                .map(|call_index| call_index + 1)
                .unwrap_or(0)
        };
        let from_index = if req.from_index < 0 {
            inferred_from_index
        } else {
            req.from_index.max(0)
        };

        let resume_execution_id = uuid::Uuid::new_v4();
        let resume_request_id = uuid::Uuid::new_v4();

        let recorded_checkpoints = checkpoint_rows
            .iter()
            .filter(|(call_index, _, _, _, _, _, _)| *call_index < from_index)
            .map(
                |(call_index, boundary, url, method, request_json, response_json, duration_ms)| {
                    serde_json::json!({
                        "call_index": *call_index as u32,
                        "boundary": boundary,
                        "url": url
                            .clone()
                            .or_else(|| {
                                request_json
                                    .get("url")
                                    .and_then(|value| value.as_str())
                                    .map(|value| value.to_string())
                            })
                            .unwrap_or_else(|| "unknown".to_string()),
                        "method": method
                            .clone()
                            .or_else(|| {
                                request_json
                                    .get("method")
                                    .and_then(|value| value.as_str())
                                    .map(|value| value.to_string())
                            })
                            .unwrap_or_else(|| "GET".to_string()),
                        "request": request_json,
                        "response": response_json,
                        "duration_ms": *duration_ms,
                    })
                },
            )
            .collect::<Vec<_>>();

        let result = perform_runtime_resume(
            &source_request_json,
            recorded_checkpoints.clone(),
            &auth_token,
        )
        .await?;

        let recorded_call_indexes = recorded_checkpoints
            .iter()
            .filter_map(|checkpoint| {
                checkpoint
                    .get("call_index")
                    .and_then(|value| value.as_i64())
                    .map(|value| value as i32)
            })
            .collect::<std::collections::HashSet<_>>();

        let checkpoints = result
            .get("checkpoints")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let logs = result
            .get("logs")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();
        let request_id = result
            .get("request_id")
            .and_then(|value| value.as_str())
            .and_then(|value| uuid::Uuid::parse_str(value).ok())
            .unwrap_or(resume_request_id);
        let result_status = result
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("error")
            .to_string();
        let result_body = result
            .get("body")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        let result_error = result
            .get("error")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string())
            .unwrap_or_default();
        let result_duration_ms = result
            .get("duration_ms")
            .and_then(|value| value.as_i64())
            .unwrap_or_default() as i32;
        let result_code_version = result
            .get("code_version")
            .and_then(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
            .unwrap_or_else(|| code_sha.clone());

        let mut steps = Vec::with_capacity(checkpoints.len());

        let mut tx =
            self.pool.begin().await.map_err(|e| {
                Status::internal(format!("failed to begin resume transaction: {e}"))
            })?;

        for checkpoint in &checkpoints {
            let call_index = checkpoint
                .get("call_index")
                .and_then(|value| value.as_i64())
                .unwrap_or_default() as i32;
            let boundary = checkpoint
                .get("boundary")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string();
            let url = checkpoint
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string();
            let method = checkpoint
                .get("method")
                .and_then(|value| value.as_str())
                .unwrap_or("GET")
                .to_string();
            let request_json = checkpoint
                .get("request")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let response_json = checkpoint
                .get("response")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let duration_ms = checkpoint
                .get("duration_ms")
                .and_then(|value| value.as_i64())
                .unwrap_or_default() as i32;

            sqlx::query(
                "INSERT INTO flux.checkpoints \
                 (execution_id, call_index, org_id, boundary, url, method, request, response, duration_ms) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            )
            .bind(resume_execution_id)
            .bind(call_index)
            .bind(identity.org_id.clone())
            .bind(boundary.clone())
            .bind(url.clone())
            .bind(method)
            .bind(request_json)
            .bind(response_json)
            .bind(duration_ms)
            .execute(&mut *tx)
            .await
            .map_err(|e| Status::internal(format!("failed to persist resume checkpoint: {e}")))?;

            steps.push(pb::ReplayStep {
                call_index,
                boundary,
                url,
                used_recorded: recorded_call_indexes.contains(&call_index),
                duration_ms,
                source: if recorded_call_indexes.contains(&call_index) {
                    "recorded".to_string()
                } else {
                    "live".to_string()
                },
                validated: false,
            });
        }

        let output_json =
            serde_json::to_string(&result_body).unwrap_or_else(|_| "null".to_string());

        sqlx::query(
            "INSERT INTO flux.executions \
             (id, request_id, org_id, project_id, method, path, status, request, response, error, code_sha, duration_ms, token_id) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NULLIF($10, ''), $11, $12, $13)",
        )
        .bind(resume_execution_id)
        .bind(request_id)
        .bind(identity.org_id.clone())
        .bind(identity.project_id.clone())
        .bind(method)
        .bind(path)
        .bind(result_status.clone())
        .bind(source_request_json)
        .bind(result_body.clone())
        .bind(result_error.clone())
        .bind(result_code_version)
        .bind(result_duration_ms)
        .bind(identity.token_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| Status::internal(format!("failed to persist resume execution: {e}")))?;

        for (seq, log) in logs.iter().enumerate() {
            sqlx::query(
                "INSERT INTO flux.execution_console_logs \
                 (execution_id, seq, org_id, level, message) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(resume_execution_id)
            .bind(seq as i32)
            .bind(identity.org_id.clone())
            .bind(
                log.get("level")
                    .and_then(|value| value.as_str())
                    .unwrap_or("info"),
            )
            .bind(
                log.get("message")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| Status::internal(format!("failed to persist resume logs: {e}")))?;
        }

        tx.commit()
            .await
            .map_err(|e| Status::internal(format!("failed to commit resume execution: {e}")))?;

        Ok(Response::new(pb::ResumeResponse {
            execution_id: resume_execution_id.to_string(),
            status: result_status,
            output: output_json,
            error: result_error,
            duration_ms: result_duration_ms,
            from_index,
            steps,
        }))
    }

    async fn ping_tail(
        &self,
        request: Request<pb::PingTailRequest>,
    ) -> Result<Response<pb::PingTailResponse>, Status> {
        let identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let payload = serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "org_id": identity.org_id,
            "project_id": req.project_id,
            "method": "PING",
            "path": "/ping",
            "status": "ok",
            "duration_ms": 0,
            "error": null
        })
        .to_string();

        let mut conn = self
            .pool
            .acquire()
            .await
            .map_err(|e| Status::internal(format!("failed to acquire connection: {e}")))?;

        // Using a raw execute on a single connection to avoid pool-level
        // issues with immediate NotificationResponse messages.
        let _ = sqlx::query("SELECT pg_notify('flux_executions', $1)")
            .bind(payload)
            .execute(&mut *conn)
            .await;

        Ok(Response::new(pb::PingTailResponse { ok: true }))
    }

    async fn deploy_function(
        &self,
        request: Request<pb::DeployFunctionRequest>,
    ) -> Result<Response<pb::DeployFunctionResponse>, Status> {
        let _identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();

        let project_id = uuid::Uuid::parse_str(&req.project_id)
            .map_err(|e| Status::invalid_argument(format!("invalid project_id: {e}")))?;

        // 1. Parse artifact to get SHA
        let artifact: shared::project::FluxBuildArtifact = serde_json::from_str(&req.artifact_json)
            .map_err(|e| Status::invalid_argument(format!("invalid artifact_json: {e}")))?;
        let artifact_id = artifact.graph_sha256;

        // 2. Insert or Update function
        let mut tx =
            self.pool.begin().await.map_err(|e| {
                Status::internal(format!("failed to begin deploy transaction: {e}"))
            })?;

        let function_id: uuid::Uuid = sqlx::query_scalar(
            "INSERT INTO control.functions (project_id, name, latest_artifact_id) \
             VALUES ($1, $2, $3) \
             ON CONFLICT (project_id, name) DO UPDATE SET \
               latest_artifact_id = EXCLUDED.latest_artifact_id, \
               created_at = now() \
             RETURNING id",
        )
        .bind(project_id)
        .bind(&req.name)
        .bind(&artifact_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| Status::internal(format!("failed to upsert function: {e}")))?;

        // 3. Upsert route
        sqlx::query(
            "INSERT INTO control.routes (project_id, method, path, function_id) \
             VALUES ($1, 'GET', $2, $3) \
             ON CONFLICT (project_id, method, path) DO UPDATE SET \
               function_id = EXCLUDED.function_id",
        )
        .bind(project_id)
        .bind(format!("/api/{}", req.name))
        .bind(function_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| Status::internal(format!("failed to upsert route: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Status::internal(format!("failed to commit deploy: {e}")))?;

        Ok(Response::new(pb::DeployFunctionResponse {
            ok: true,
            function_id: function_id.to_string(),
            message: format!(
                "Function '{}' deployed successfully with artifact {}",
                req.name,
                &artifact_id[..8]
            ),
        }))
    }

    async fn list_functions(
        &self,
        request: Request<pb::ListFunctionsRequest>,
    ) -> Result<Response<pb::ListFunctionsResponse>, Status> {
        let _identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let project_id = uuid::Uuid::parse_str(&req.project_id)
            .map_err(|e| Status::invalid_argument(format!("invalid project_id: {e}")))?;

        let rows: Vec<(uuid::Uuid, String, Option<chrono::DateTime<chrono::Utc>>)> =
            sqlx::query_as(
                "SELECT id, name, created_at FROM control.functions WHERE project_id = $1",
            )
            .bind(project_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("failed to list functions: {e}")))?;

        let functions = rows
            .into_iter()
            .map(|(id, name, created_at)| pb::FunctionEntry {
                id: id.to_string(),
                name,
                created_at: created_at.map(|t| t.to_rfc3339()).unwrap_or_default(),
            })
            .collect();

        Ok(Response::new(pb::ListFunctionsResponse { functions }))
    }

    async fn delete_function(
        &self,
        request: Request<pb::DeleteFunctionRequest>,
    ) -> Result<Response<pb::DeleteFunctionResponse>, Status> {
        let _identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let function_id = uuid::Uuid::parse_str(&req.function_id)
            .map_err(|e| Status::invalid_argument(format!("invalid function_id: {e}")))?;

        sqlx::query("DELETE FROM control.functions WHERE id = $1")
            .bind(function_id)
            .execute(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("failed to delete function: {e}")))?;

        Ok(Response::new(pb::DeleteFunctionResponse { ok: true }))
    }

    async fn list_env_vars(
        &self,
        request: Request<pb::ListEnvVarsRequest>,
    ) -> Result<Response<pb::ListEnvVarsResponse>, Status> {
        let _identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let project_id = uuid::Uuid::parse_str(&req.project_id)
            .map_err(|e| Status::invalid_argument(format!("invalid project_id: {e}")))?;

        let rows: Vec<(String, String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
            "SELECT key, value, updated_at FROM control.env_vars WHERE project_id = $1",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to list env vars: {e}")))?;

        let env_vars = rows
            .into_iter()
            .map(|(key, value, updated_at)| pb::EnvVarEntry {
                key,
                value,
                updated_at: updated_at.to_rfc3339(),
            })
            .collect();

        Ok(Response::new(pb::ListEnvVarsResponse { env_vars }))
    }

    async fn set_env_var(
        &self,
        request: Request<pb::SetEnvVarRequest>,
    ) -> Result<Response<pb::SetEnvVarResponse>, Status> {
        let _identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let project_id = uuid::Uuid::parse_str(&req.project_id)
            .map_err(|e| Status::invalid_argument(format!("invalid project_id: {e}")))?;

        sqlx::query(
            "INSERT INTO control.env_vars (project_id, key, value, updated_at) \
             VALUES ($1, $2, $3, now()) \
             ON CONFLICT (project_id, key) DO UPDATE SET \
               value = EXCLUDED.value, \
               updated_at = now()",
        )
        .bind(project_id)
        .bind(req.key)
        .bind(req.value)
        .execute(&self.pool)
        .await
        .map_err(|e| Status::internal(format!("failed to set env var: {e}")))?;

        Ok(Response::new(pb::SetEnvVarResponse { ok: true }))
    }

    async fn delete_env_var(
        &self,
        request: Request<pb::DeleteEnvVarRequest>,
    ) -> Result<Response<pb::DeleteEnvVarResponse>, Status> {
        let _identity = self.authenticate(request.metadata()).await?;
        let req = request.into_inner();
        let project_id = uuid::Uuid::parse_str(&req.project_id)
            .map_err(|e| Status::invalid_argument(format!("invalid project_id: {e}")))?;

        sqlx::query("DELETE FROM control.env_vars WHERE project_id = $1 AND key = $2")
            .bind(project_id)
            .bind(req.key)
            .execute(&self.pool)
            .await
            .map_err(|e| Status::internal(format!("failed to delete env var: {e}")))?;

        Ok(Response::new(pb::DeleteEnvVarResponse { ok: true }))
    }
}

pub async fn serve(
    addr: SocketAddr,
    service: InternalAuthGrpc,
    mut shutdown_rx: watch::Receiver<()>,
) -> Result<(), tonic::transport::Error> {
    tonic::transport::Server::builder()
        .http2_keepalive_interval(Some(std::time::Duration::from_secs(10)))
        .http2_keepalive_timeout(Some(std::time::Duration::from_secs(20)))
        .add_service(pb::internal_auth_service_server::InternalAuthServiceServer::new(service))
        .serve_with_shutdown(addr, async move {
            let _ = shutdown_rx.changed().await;
        })
        .await
}
