//! HTTP implementation of [`QueueDispatch`].
//!
//! Wraps the `POST {queue_url}/jobs` call (currently inlined inside the V8
//! isolate `op_queue_push` op).  Will be used by the server crate's in-process
//! queue dispatch once the op is refactored to accept a trait object.

use async_trait::async_trait;
use serde_json::Value;

use job_contract::dispatch::QueueDispatch;

/// Pushes jobs to a remote queue service over HTTP.
pub struct HttpQueueDispatch {
    pub client:    reqwest::Client,
    pub queue_url: String,
    pub token:     String,
}

#[async_trait]
impl QueueDispatch for HttpQueueDispatch {
    async fn push_job(
        &self,
        function_id: &str,
        payload:     Value,
        delay_seconds: Option<u64>,
        idempotency_key: Option<String>,
    ) -> Result<(), String> {
        let url = format!("{}/jobs", self.queue_url.trim_end_matches('/'));

        let mut body = serde_json::json!({
            "function_id": function_id,
            "payload":     payload,
        });
        if let Some(d) = delay_seconds {
            body["delay_seconds"] = serde_json::json!(d);
        }
        if let Some(k) = idempotency_key {
            body["idempotency_key"] = serde_json::json!(k);
        }

        let resp = self.client
            .post(&url)
            .header("X-Service-Token", &self.token)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("queue push failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body   = resp.text().await.unwrap_or_default();
            return Err(format!("queue service error HTTP {}: {}", status, body));
        }

        Ok(())
    }
}
