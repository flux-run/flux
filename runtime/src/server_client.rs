use anyhow::{Context, Result};
use tonic::metadata::MetadataValue;
use tonic::Request;

use crate::deno_runtime::{FetchCheckpoint, LogEntry};
use crate::isolate_pool::ExecutionResult;

pub use shared::pb;

pub struct ExecutionEnvelope {
    pub method: String,
    pub path: String,
    pub project_id: Option<String>,
    pub request_json: serde_json::Value,
    pub result: ExecutionResult,
}

#[derive(Debug, Clone)]
pub struct CompletedExecutionCacheHit {
    pub execution_id: String,
    pub request_id: String,
    pub code_version: String,
    pub status: String,
    pub response_json: serde_json::Value,
    pub error: String,
    pub duration_ms: i32,
    pub response_status: i32,
    pub response_body: String,
    pub error_name: String,
    pub error_message: String,
    pub error_stack: String,
    pub error_phase: String,
    pub error_source: String,
    pub error_type: String,
    pub is_user_code: bool,
    pub error_frames: serde_json::Value,
    pub attempt: i32,
}

#[derive(Debug, Clone)]
pub struct LatestExecutionState {
    pub execution_id: String,
    pub request_id: String,
    pub status: String,
    pub attempt: i32,
    pub retry_after_ms: i32,
}

pub async fn record_execution(url: &str, token: &str, envelope: ExecutionEnvelope) -> Result<()> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .with_context(|| format!("failed to connect to Flux server at {}", endpoint))?;

    let mut request = Request::new(pb::RecordExecutionRequest {
        execution_id: envelope.result.execution_id,
        request_id: envelope.result.request_id,
        code_version: envelope.result.code_version,
        method: envelope.method,
        path: envelope.path,
        status: envelope.result.status,
        request_json: serde_json::to_string(&envelope.request_json)
            .context("failed to encode request JSON")?,
        response_json: serde_json::to_string(&envelope.result.body)
            .context("failed to encode response JSON")?,
        error: envelope.result.error.unwrap_or_default(),
        duration_ms: envelope.result.duration_ms,
        project_id: envelope.project_id.unwrap_or_default(),
        checkpoints: envelope
            .result
            .checkpoints
            .into_iter()
            .map(checkpoint_to_proto)
            .collect(),
        logs: envelope
            .result
            .logs
            .into_iter()
            .map(log_entry_to_proto)
            .collect(),

        // Advanced Observability
        client_ip: envelope.result.client_ip.unwrap_or_default(),
        user_agent: envelope.result.user_agent.unwrap_or_default(),
        request_method: envelope.result.request_method.unwrap_or_default(),
        request_headers_json: serde_json::to_string(
            &envelope
                .result
                .request_headers
                .unwrap_or(serde_json::Value::Null),
        )
        .unwrap_or_default(),
        request_body: envelope.result.request_body.unwrap_or_default(),
        response_status: envelope.result.response_status.unwrap_or(0),
        response_body: envelope.result.response_body.unwrap_or_default(),
        error_stack: envelope.result.error_stack.unwrap_or_default(),
        error_fingerprint: envelope.result.error_fingerprint.unwrap_or_default(),
        error_source: envelope.result.error_source.unwrap_or_default(),
        error_type: envelope.result.error_type.unwrap_or_default(),
        error_name: envelope.result.error_name.unwrap_or_default(),
        error_message: envelope.result.error_message.unwrap_or_default(),
        error_phase: envelope.result.error_phase.unwrap_or_default(),
        is_user_code: envelope.result.is_user_code.unwrap_or(false),
        error_frames_json: serde_json::to_string(
            &envelope.result.error_frames.unwrap_or(serde_json::Value::Null),
        )
        .unwrap_or_default(),
    });

    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    client
        .record_execution(request)
        .await
        .context("record execution request failed")?;

    Ok(())
}

pub async fn get_completed_execution_by_request(
    url: &str,
    token: &str,
    request_id: &str,
) -> Result<Option<CompletedExecutionCacheHit>> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .with_context(|| format!("failed to connect to Flux server at {}", endpoint))?;

    let mut request = Request::new(pb::GetCompletedExecutionByRequestRequest {
        request_id: request_id.to_string(),
    });

    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .get_completed_execution_by_request(request)
        .await
        .context("get completed execution by request request failed")?
        .into_inner();

    if !response.found {
        return Ok(None);
    }

    Ok(Some(CompletedExecutionCacheHit {
        execution_id: response.execution_id,
        request_id: response.request_id,
        code_version: response.code_version,
        status: response.status,
        response_json: serde_json::from_str(&response.response_json)
            .unwrap_or(serde_json::Value::Null),
        error: response.error,
        duration_ms: response.duration_ms,
        response_status: response.response_status,
        response_body: response.response_body,
        error_name: response.error_name,
        error_message: response.error_message,
        error_stack: response.error_stack,
        error_phase: response.error_phase,
        error_source: response.error_source,
        error_type: response.error_type,
        is_user_code: response.is_user_code,
        error_frames: serde_json::from_str(&response.error_frames_json)
            .unwrap_or(serde_json::Value::Null),
        attempt: response.attempt,
    }))
}

pub async fn get_latest_execution_by_request(
    url: &str,
    token: &str,
    request_id: &str,
) -> Result<Option<LatestExecutionState>> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .with_context(|| format!("failed to connect to Flux server at {}", endpoint))?;

    let mut request = Request::new(pb::GetLatestExecutionByRequestRequest {
        request_id: request_id.to_string(),
    });

    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .get_latest_execution_by_request(request)
        .await
        .context("get latest execution by request request failed")?
        .into_inner();

    if !response.found {
        return Ok(None);
    }

    Ok(Some(LatestExecutionState {
        execution_id: response.execution_id,
        request_id: response.request_id,
        status: response.status,
        attempt: response.attempt,
        retry_after_ms: response.retry_after_ms,
    }))
}

pub async fn claim_execution(
    url: &str,
    token: &str,
    execution_id: &str,
    request_id: &str,
    attempt: i32,
) -> Result<bool> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
            .await
            .with_context(|| format!("failed to connect to Flux server at {}", endpoint))?;

    let mut request = Request::new(pb::ClaimExecutionRequest {
        execution_id: execution_id.to_string(),
        request_id: request_id.to_string(),
        attempt,
    });

    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .claim_execution(request)
        .await
        .context("claim_execution request failed")?
        .into_inner();

    Ok(response.claimed)
}

pub async fn get_trace(url: &str, token: &str, execution_id: &str) -> Result<pb::GetTraceResponse> {
    let endpoint = normalize_grpc_url(url);
    let mut client =
        pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
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
        .context("get trace request failed")?;

    Ok(response.into_inner())
}

fn checkpoint_to_proto(checkpoint: FetchCheckpoint) -> pb::CheckpointEntry {
    pb::CheckpointEntry {
        call_index: checkpoint.call_index,
        boundary: checkpoint.boundary,
        url: checkpoint.url,
        method: checkpoint.method,
        request_json: serde_json::to_string(&checkpoint.request)
            .unwrap_or_else(|_| "null".to_string()),
        response_json: serde_json::to_string(&checkpoint.response)
            .unwrap_or_else(|_| "null".to_string()),
        duration_ms: checkpoint.duration_ms,
    }
}

fn log_entry_to_proto(entry: LogEntry) -> pb::ConsoleLogEntry {
    pb::ConsoleLogEntry {
        level: entry.level,
        message: entry.message,
        seq: entry.call_index,
    }
}

fn normalize_grpc_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    }
}
