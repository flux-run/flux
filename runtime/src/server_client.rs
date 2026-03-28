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
