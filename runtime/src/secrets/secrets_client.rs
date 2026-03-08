use reqwest::{Client, StatusCode};
use std::collections::HashMap;
use uuid::Uuid;
use crate::config::settings::Settings;

#[derive(Clone)]
pub struct SecretsClient {
    client: Client,
    settings: Settings,
}

impl SecretsClient {
    pub fn new(settings: Settings) -> Self {
        Self {
            client: Client::new(),
            settings,
        }
    }

    pub async fn fetch_secrets(
        &self,
        tenant_id: Uuid,
        project_id: Option<Uuid>,
    ) -> Result<HashMap<String, String>, String> {
        let mut url = format!("{}/internal/secrets?tenant_id={}", self.settings.control_plane_url, tenant_id);
        if let Some(pid) = project_id {
            url.push_str(&format!("&project_id={}", pid));
        }

        let resp = self
            .client
            .get(&url)
            .header("X-Service-Token", &self.settings.service_token)
            .send()
            .await
            .map_err(|e| format!("Failed to fetch secrets: {}", e))?;

        let status = resp.status();
        if status != StatusCode::OK {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(format!("Control plane error HTTP {}: {}", status, error_text));
        }

        // Control plane now returns ApiResponse<T>: { success: true, data: {...} }
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("Failed parsing secrets JSON: {}", e))?;

        // Extract the `data` wrapper if present (ApiResponse), else use root directly
        let secrets_value = json.get("data").cloned().unwrap_or(json);

        let secrets_map: HashMap<String, String> = serde_json::from_value(secrets_value)
            .map_err(|e| format!("Failed deserializing secrets map: {}", e))?;

        Ok(secrets_map)
    }
}
