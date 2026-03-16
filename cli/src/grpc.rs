use anyhow::{Context, Result, bail};
use tonic::Request;
use tonic::metadata::MetadataValue;

pub mod pb {
    tonic::include_proto!("flux.internal.v1");
}

#[derive(Debug, Clone)]
pub struct LogEntry {
    pub request_id: String,
    pub code_version: String,
    pub status: String,
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
pub struct TraceView {
    pub execution_id: String,
    pub method: String,
    pub path: String,
    pub status: String,
    pub duration_ms: i32,
    pub error: String,
    pub checkpoints: Vec<TraceCheckpoint>,
}

pub async fn validate_service_token(url: &str, token: &str) -> Result<String> {
    let endpoint = normalize_grpc_url(url);
    let mut client = pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
        .await
        .with_context(|| format!("failed to connect to Flux server at {}", endpoint))?;

    let mut request = Request::new(pb::ValidateTokenRequest {});
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .validate_token(request)
        .await
        .context("service token validation failed")?
        .into_inner();

    if !response.ok {
        bail!("service token was rejected by the server");
    }

    Ok(response.auth_mode)
}

pub async fn list_logs(url: &str, token: &str, limit: u32) -> Result<Vec<LogEntry>> {
    let endpoint = normalize_grpc_url(url);
    let mut client = pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
        .await
        .with_context(|| format!("failed to connect to Flux server at {}", endpoint))?;

    let mut request = Request::new(pb::ListLogsRequest { limit });
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .list_logs(request)
        .await
        .context("list logs request failed")?
        .into_inner();

    Ok(response
        .logs
        .into_iter()
        .map(|log| LogEntry {
            request_id: log.request_id,
            code_version: log.code_version,
            status: log.status,
            timestamp: log.timestamp,
        })
        .collect())
}

pub async fn get_trace(url: &str, token: &str, execution_id: &str) -> Result<TraceView> {
    let endpoint = normalize_grpc_url(url);
    let mut client = pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
        .await
        .with_context(|| format!("failed to connect to Flux server at {}", endpoint))?;

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
        .context("get trace request failed")?
        .into_inner();

    Ok(TraceView {
        execution_id: response.execution_id,
        method: response.method,
        path: response.path,
        status: response.status,
        duration_ms: response.duration_ms,
        error: response.error,
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
    })
}

pub fn normalize_grpc_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    }
}