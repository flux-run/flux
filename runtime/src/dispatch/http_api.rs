//! HTTP implementation of [`ApiDispatch`].
//!
//! Wraps the existing control-plane HTTP calls (bundle, logs, secrets) that
//! the runtime makes to the API service.  Used in multi-process mode.
//! The server crate provides an in-process alternative.

use async_trait::async_trait;
use std::collections::HashMap;
use serde_json::Value;
use uuid::Uuid;

use job_contract::dispatch::ApiDispatch;

/// Makes HTTP calls to a remote API service.
pub struct HttpApiDispatch {
    pub client:    reqwest::Client,
    pub api_url:   String,
    pub token:     String,
}

#[async_trait]
impl ApiDispatch for HttpApiDispatch {
    async fn get_bundle(&self, function_id: &str) -> Result<Value, String> {
        let url = format!(
            "{}/internal/bundle?function_id={}",
            self.api_url, function_id
        );

        let resp = self.client
            .get(&url)
            .header("X-Service-Token", &self.token)
            .send()
            .await
            .map_err(|e| format!("bundle fetch failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body   = resp.text().await.unwrap_or_default();
            return Err(format!("API bundle error HTTP {}: {}", status, body));
        }

        // API returns ApiResponse<T>: { success: true, data: ... }
        let json: Value = resp.json().await
            .map_err(|e| format!("bundle JSON parse failed: {}", e))?;

        Ok(json.get("data").cloned().unwrap_or(json))
    }

    async fn write_log(&self, entry: Value) -> Result<(), String> {
        let url = format!("{}/internal/logs", self.api_url);

        let resp = self.client
            .post(&url)
            .header("X-Service-Token", &self.token)
            .json(&entry)
            .send()
            .await
            .map_err(|e| format!("log write failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body   = resp.text().await.unwrap_or_default();
            return Err(format!("API log error HTTP {}: {}", status, body));
        }

        Ok(())
    }

    async fn get_secrets(
        &self,
        project_id: Option<Uuid>,
    ) -> Result<HashMap<String, String>, String> {
        let mut url = format!("{}/internal/secrets", self.api_url);
        if let Some(pid) = project_id {
            url.push_str(&format!("?project_id={}", pid));
        }

        let resp = self.client
            .get(&url)
            .header("X-Service-Token", &self.token)
            .send()
            .await
            .map_err(|e| format!("secrets fetch failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body   = resp.text().await.unwrap_or_default();
            return Err(format!("API secrets error HTTP {}: {}", status, body));
        }

        let json: Value = resp.json().await
            .map_err(|e| format!("secrets JSON parse failed: {}", e))?;

        let map_val = json.get("data").cloned().unwrap_or(json);
        serde_json::from_value::<HashMap<String, String>>(map_val)
            .map_err(|e| format!("secrets deserialize failed: {}", e))
    }
}
