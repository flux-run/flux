use anyhow::{Context, Result, bail};
use tonic::Request;
use tonic::Streaming;
use tonic::metadata::MetadataValue;

pub use shared::pb;

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub execution_id: String,
    pub request_id: String,
    pub method: String,
    pub path: String,
    pub code_version: String,
    pub status: String,
    pub duration_ms: i32,
    pub error: String,
    pub timestamp: String,
}

#[derive(Debug, Clone)]
pub struct TraceCheckpoint {
    pub call_index: i32,
    pub boundary: String,
    pub request: Vec<u8>,
    pub response: Vec<u8>,
    pub duration_ms: i32,
}

#[derive(Debug, Clone)]
pub struct TraceConsoleLog {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone)]
pub struct TraceView {
    pub execution_id: String,
    pub method: String,
    pub path: String,
    pub status: String,
    pub duration_ms: i32,
    pub error: String,
    pub request_json: String,
    pub response_json: String,
    pub checkpoints: Vec<TraceCheckpoint>,
    pub logs: Vec<TraceConsoleLog>,
}

fn friendly_connect_error(endpoint: &str, err: tonic::transport::Error) -> anyhow::Error {
    anyhow::anyhow!(
        "server not running\n\nfailed to connect to {} ({})\n\nstart it with:\n  flux server start --database-url postgres://...",
        endpoint,
        err,
    )
}

fn friendly_status_error(action: &str, err: tonic::Status) -> anyhow::Error {
    if err.code() == tonic::Code::Unauthenticated {
        anyhow::anyhow!(
            "authentication failed\n\ncheck your token with:\n  flux config get token\n\nreset with:\n  flux auth --url <host:port>"
        )
    } else {
        anyhow::anyhow!("{} failed: {}", action, err.message())
    }
}

#[derive(Debug, Clone)]
pub struct WhyView {
    pub execution_id: String,
    pub method: String,
    pub path: String,
    pub status: String,
    pub duration_ms: i32,
    pub reason: String,
    pub suggestion: String,
    pub error_body: String,
    pub logs: Vec<(String, String)>, // (level, message)
}

#[derive(Debug, Clone)]
pub struct ReplayStepView {
    pub call_index: i32,
    pub boundary: String,
    pub url: String,
    pub used_recorded: bool,
    pub duration_ms: i32,
    pub source: String,
    pub validated: bool,
}

#[derive(Debug, Clone)]
pub struct ReplayDivergenceView {
    pub checkpoint_index: i32,
    pub boundary: String,
    pub url: String,
    pub expected_json: String,
    pub actual_json: String,
    pub diffs: Vec<ReplayFieldDiffView>,
}

#[derive(Debug, Clone)]
pub struct ReplayFieldDiffView {
    pub path: String,
    pub expected_json: String,
    pub actual_json: String,
    pub kind: String,
}

#[derive(Debug, Clone)]
pub struct ReplayView {
    pub execution_id: String,
    pub status: String,
    pub output: String,
    pub error: String,
    pub duration_ms: i32,
    pub steps: Vec<ReplayStepView>,
    pub divergence: Option<ReplayDivergenceView>,
}

#[derive(Debug, Clone)]
pub struct ResumeView {
    pub execution_id: String,
    pub status: String,
    pub output: String,
    pub error: String,
    pub duration_ms: i32,
    pub from_index: i32,
    pub steps: Vec<ReplayStepView>,
}

#[derive(Debug, Clone)]
pub struct AuthResult {
    pub auth_mode: String,
    pub project_id: Option<String>,
}

pub async fn validate_service_token(url: &str, token: &str) -> Result<AuthResult> {
    let endpoint = normalize_grpc_url(url);

    // If it's an HTTPS URL, we try the REST fallback first or as a fallback
    // because cloud environments often have protocol issues with pure gRPC over shared infrastructure.
    if endpoint.starts_with("https://") {
        println!("  Attempting cloud authentication...");
        match validate_service_token_rest(&endpoint, token).await {
            Ok(result) => return Ok(result),
            Err(e) => {
                println!("  Cloud authentication failed: {}. Falling back to gRPC...", e);
            }
        }
    }

    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::ValidateTokenRequest {});
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .validate_token(request)
        .await
        .map_err(|e| friendly_status_error("service token validation", e))?
        .into_inner();

    if !response.ok {
        bail!("service token was rejected by the server");
    }

    Ok(AuthResult {
        auth_mode: response.auth_mode,
        project_id: if response.project_id.is_empty() { None } else { Some(response.project_id) },
    })
}

async fn validate_service_token_rest(url: &str, token: &str) -> Result<AuthResult> {
    let client = reqwest::Client::new();
    let validate_url = format!("{}/auth/validate", url.trim_end_matches('/'));

    let response = client
        .get(&validate_url)
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await
        .context("failed to send REST validation request")?;

    let status = response.status();
    if !status.is_success() {
        let err_body = response.text().await.unwrap_or_default();
        bail!("REST validation failed ({}): {}", status, err_body);
    }

    let body: serde_json::Value = response.json().await.context("failed to parse REST validation response")?;
    let auth_mode = body["auth_mode"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("REST response missing auth_mode"))?;
    
    let project_id = body["project_id"].as_str().map(|s| s.to_string());

    Ok(AuthResult {
        auth_mode: auth_mode.to_string(),
        project_id,
    })
}

pub async fn list_logs(url: &str, token: &str, limit: u32) -> Result<Vec<LogEntry>> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::ListLogsRequest { limit });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .list_logs(request)
        .await
        .map_err(|e| friendly_status_error("logs request", e))?
        .into_inner();

    Ok(response
        .logs
        .into_iter()
        .map(|log| LogEntry {
            execution_id: log.execution_id,
            request_id: log.request_id,
            method: log.method,
            path: log.path,
            code_version: log.code_version,
            status: log.status,
            duration_ms: log.duration_ms,
            error: log.error,
            timestamp: log.timestamp,
        })
        .collect())
}

pub async fn get_trace(url: &str, token: &str, execution_id: &str) -> Result<TraceView> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::GetTraceRequest {
        execution_id: execution_id.to_string(),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .get_trace(request)
        .await
        .map_err(|e| friendly_status_error("trace request", e))?
        .into_inner();

    Ok(TraceView {
        execution_id: response.execution_id,
        method: response.method,
        path: response.path,
        status: response.status,
        duration_ms: response.duration_ms,
        error: response.error,
        request_json: response.request_json,
        response_json: response.response_json,
        checkpoints: response
            .checkpoints
            .into_iter()
            .map(|cp| TraceCheckpoint {
                call_index: cp.call_index,
                boundary: cp.boundary,
                request: cp.request,
                response: cp.response,
                duration_ms: cp.duration_ms,
            })
            .collect(),
        logs: response
            .logs
            .into_iter()
            .map(|log| TraceConsoleLog {
                level: log.level,
                message: log.message,
            })
            .collect(),
    })
}

pub async fn tail(
    url: &str,
    token: &str,
    project_id: Option<String>,
) -> Result<Streaming<pb::TailEvent>> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::TailRequest {
        project_id: project_id.unwrap_or_default(),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .tail(request)
        .await
        .map_err(|e| friendly_status_error("tail request", e))?
        .into_inner();

    Ok(response)
}

pub async fn why(url: &str, token: &str, execution_id: &str) -> Result<WhyView> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::WhyRequest {
        execution_id: execution_id.to_string(),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .why(request)
        .await
        .map_err(|e| friendly_status_error("why request", e))?
        .into_inner();

    Ok(WhyView {
        execution_id: response.execution_id,
        method: response.method,
        path: response.path,
        status: response.status,
        duration_ms: response.duration_ms,
        reason: response.reason,
        suggestion: response.suggestion,
        error_body: response.error_body,
        logs: response.logs.into_iter().map(|l| (l.level, l.message)).collect(),
    })
}

pub async fn replay(
    url: &str,
    token: &str,
    execution_id: &str,
    commit: bool,
    from_index: i32,
    validate: bool,
) -> Result<ReplayView> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::ReplayRequest {
        execution_id: execution_id.to_string(),
        commit,
        from_index,
        validate,
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .replay(request)
        .await
        .map_err(|e| friendly_status_error("replay request", e))?
        .into_inner();

    Ok(ReplayView {
        execution_id: response.execution_id,
        status: response.status,
        output: response.output,
        error: response.error,
        duration_ms: response.duration_ms,
        steps: response
            .steps
            .into_iter()
            .map(|step| ReplayStepView {
                call_index: step.call_index,
                boundary: step.boundary,
                url: step.url,
                used_recorded: step.used_recorded,
                duration_ms: step.duration_ms,
                source: step.source,
                validated: step.validated,
            })
            .collect(),
        divergence: response.divergence.map(|divergence| ReplayDivergenceView {
            checkpoint_index: divergence.checkpoint_index,
            boundary: divergence.boundary,
            url: divergence.url,
            expected_json: divergence.expected_json,
            actual_json: divergence.actual_json,
            diffs: divergence
                .diffs
                .into_iter()
                .map(|diff| ReplayFieldDiffView {
                    path: diff.path,
                    expected_json: diff.expected_json,
                    actual_json: diff.actual_json,
                    kind: diff.kind,
                })
                .collect(),
        }),
    })
}

pub async fn resume(
    url: &str,
    token: &str,
    execution_id: &str,
    from_index: Option<i32>,
) -> Result<ResumeView> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::ResumeRequest {
        execution_id: execution_id.to_string(),
        from_index: from_index.unwrap_or(-1),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .resume(request)
        .await
        .map_err(|e| friendly_status_error("resume request", e))?
        .into_inner();

    Ok(ResumeView {
        execution_id: response.execution_id,
        status: response.status,
        output: response.output,
        error: response.error,
        duration_ms: response.duration_ms,
        from_index: response.from_index,
        steps: response
            .steps
            .into_iter()
            .map(|step| ReplayStepView {
                call_index: step.call_index,
                boundary: step.boundary,
                url: step.url,
                used_recorded: step.used_recorded,
                duration_ms: step.duration_ms,
                source: step.source,
                validated: step.validated,
            })
            .collect(),
    })
}

pub async fn ping_tail(url: &str, token: &str, project_id: Option<String>) -> Result<()> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::PingTailRequest {
        project_id: project_id.unwrap_or_default(),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    client
        .ping_tail(request)
        .await
        .map_err(|e| friendly_status_error("ping-tail request", e))?;

    Ok(())
}

pub async fn deploy_function(
    url: &str,
    token: &str,
    project_id: &str,
    name: &str,
    artifact_json: &str,
) -> Result<pb::DeployFunctionResponse> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::DeployFunctionRequest {
        project_id: project_id.to_string(),
        name: name.to_string(),
        artifact_json: artifact_json.to_string(),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .deploy_function(request)
        .await
        .map_err(|e| friendly_status_error("deploy request", e))?
        .into_inner();

    Ok(response)
}

pub async fn list_functions(url: &str, token: &str, project_id: &str) -> Result<Vec<pb::FunctionEntry>> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::ListFunctionsRequest {
        project_id: project_id.to_string(),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .list_functions(request)
        .await
        .map_err(|e| friendly_status_error("list functions request", e))?
        .into_inner();

    Ok(response.functions)
}

pub async fn delete_function(url: &str, token: &str, function_id: &str) -> Result<()> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::DeleteFunctionRequest {
        function_id: function_id.to_string(),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .delete_function(request)
        .await
        .map_err(|e| friendly_status_error("delete function request", e))?
        .into_inner();

    if !response.ok {
        bail!("failed to delete function");
    }

    Ok(())
}

pub async fn list_env_vars(url: &str, token: &str, project_id: &str) -> Result<Vec<pb::EnvVarEntry>> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::ListEnvVarsRequest {
        project_id: project_id.to_string(),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .list_env_vars(request)
        .await
        .map_err(|e| friendly_status_error("list env vars request", e))?
        .into_inner();

    Ok(response.env_vars)
}

pub async fn set_env_var(url: &str, token: &str, project_id: &str, key: &str, value: &str) -> Result<()> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::SetEnvVarRequest {
        project_id: project_id.to_string(),
        key: key.to_string(),
        value: value.to_string(),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .set_env_var(request)
        .await
        .map_err(|e| friendly_status_error("set env var request", e))?
        .into_inner();

    if !response.ok {
        bail!("failed to set environment variable");
    }

    Ok(())
}

pub async fn delete_env_var(url: &str, token: &str, project_id: &str, key: &str) -> Result<()> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .map_err(|e| friendly_connect_error(&endpoint, e))?;

    let mut request = Request::new(pb::DeleteEnvVarRequest {
        project_id: project_id.to_string(),
        key: key.to_string(),
    });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .delete_env_var(request)
        .await
        .map_err(|e| friendly_status_error("delete env var request", e))?
        .into_inner();

    if !response.ok {
        bail!("failed to delete environment variable");
    }

    Ok(())
}

pub fn normalize_grpc_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    }
}
