use reqwest::{Client, header};
use serde_json::Value;

use crate::config::Config;

pub struct ApiClient {
    pub client: Client,
    pub base_url: String,
    pub config: Config,
}

impl ApiClient {
    pub async fn new() -> anyhow::Result<Self> {
        let config = Config::load().await;
        
        let token = config.token.clone()
            .ok_or_else(|| anyhow::anyhow!("Unauthenticated. Please run `flux login` first."))?;

        let mut headers = header::HeaderMap::new();
        let auth_value = header::HeaderValue::from_str(&format!("Bearer {}", token))
            .map_err(|_| anyhow::anyhow!("Invalid API configuration syntax internally"))?;
            
        headers.insert(header::AUTHORIZATION, auth_value);

        if let Some(tenant_id) = &config.tenant_id {
            if let Ok(tenant_val) = header::HeaderValue::from_str(tenant_id) {
                headers.insert("X-Fluxbase-Tenant", tenant_val);
            }
        }

        if let Some(project_id) = &config.project_id {
            if let Ok(project_val) = header::HeaderValue::from_str(project_id) {
                headers.insert("X-Fluxbase-Project", project_val);
            }
        }

        let client = Client::builder()
            .default_headers(headers)
            .build()?;

        let base_url = config.api_url.clone();

        Ok(ApiClient { client, base_url, config })
    }

    pub async fn deploy_function(&self, payload: reqwest::multipart::Form) -> anyhow::Result<Value> {
        let url = format!("{}/functions/deploy", self.base_url);
        
        let response = self.client.post(&url)
            .multipart(payload)
            .send()
            .await?;

        Ok(response.error_for_status()?.json().await?)
    }
}
