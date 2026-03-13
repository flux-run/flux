//! HTTP implementation of [`RuntimeDispatch`] — wraps the existing
//! `POST {runtime_url}/execute` network call in the trait interface.
//!
//! Used by the gateway in multi-process mode.  The server crate provides an
//! alternative in-process implementation that calls into the runtime library
//! directly.

use async_trait::async_trait;
use job_contract::dispatch::{ExecuteRequest, ExecuteResponse, RuntimeDispatch};

/// Forwards execute requests to a remote runtime over HTTP.
pub struct HttpRuntimeDispatch {
    pub client:        reqwest::Client,
    pub runtime_url:   String,
    pub service_token: String,
}

#[async_trait]
impl RuntimeDispatch for HttpRuntimeDispatch {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteResponse, String> {
        let url = format!("{}/execute", self.runtime_url);

        let mut body = serde_json::json!({
            "function_id":    req.function_id,
            "project_id":     req.project_id,
            "payload":        req.payload,
        });
        if let Some(seed) = req.execution_seed {
            body["execution_seed"] = serde_json::json!(seed);
        }

        let mut builder = self.client
            .post(&url)
            .header("X-Service-Token", &self.service_token)
            .json(&body);

        if let Some(hint) = &req.runtime_hint {
            builder = builder.header("X-Function-Runtime", hint);
        }
        if let Some(rid) = &req.request_id {
            builder = builder.header("x-request-id", rid);
        }
        if let Some(span) = &req.parent_span_id {
            builder = builder.header("x-parent-span-id", span);
        }
        if let Some(uid) = &req.user_id {
            builder = builder.header("X-User-Id", uid);
        }
        if let Some(claims) = &req.jwt_claims {
            if let Ok(json) = serde_json::to_string(claims) {
                builder = builder.header("X-JWT-Claims", json);
            }
        }

        let start = std::time::Instant::now();

        let resp = builder
            .send()
            .await
            .map_err(|e| format!("runtime_unreachable: {}", e))?;

        let status  = resp.status().as_u16();
        let raw     = resp.text().await.unwrap_or_default();
        let duration_ms = start.elapsed().as_millis() as u64;

        let body: serde_json::Value = serde_json::from_str(&raw).unwrap_or_else(|_| {
            serde_json::json!({ "error": "runtime_response_parse_error", "raw": raw })
        });

        Ok(ExecuteResponse { body, status, duration_ms })
    }
}
