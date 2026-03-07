use reqwest::{Client, Error as ReqwestError, header};
use serde_json::Value;

use super::config::Config;

pub struct ApiClient {
    client: Client,
    base_url: String,
}

impl ApiClient {
    pub async fn new() -> Result<Self, String> {
        let config = Config::load().await;
        
        let api_key = config.api_key
            .ok_or("Unauthenticated. Please run `flux login` first.")?;

        let mut headers = header::HeaderMap::new();
        let auth_value = header::HeaderValue::from_str(&format!("Bearer {}", api_key))
            .map_err(|_| "Invalid API configuration syntax internally")?;
            
        headers.insert(header::AUTHORIZATION, auth_value);

        let project_id = config.project_id
            .ok_or("No project initialized. Run `flux login`.")?;

        let project_value = header::HeaderValue::from_str(&project_id)
            .map_err(|_| "Invalid project ID syntax internally")?;

        headers.insert("X-Fluxbase-Project", project_value);

        let client = Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| e.to_string())?;

        // Fallback for demonstration since we only have a local instance presently
        let base_url = std::env::var("FLUXBASE_API_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:3000".to_string());

        Ok(ApiClient { client, base_url })
    }

    pub async fn deploy_function(&self, payload: Value) -> Result<Value, ReqwestError> {
        let url = format!("{}/functions/deploy", self.base_url);
        
        let response = self.client.post(&url)
            .json(&payload)
            .send()
            .await?;

        response.error_for_status()?.json().await
    }
}
