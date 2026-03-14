use crate::client::ApiClient;
use serde_json::Value;

pub async fn execute() -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let res = client.client
        .get(format!("{}/auth/me", client.base_url))
        .send()
        .await?;
    let json: Value = res.error_for_status()?.json().await?;
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}
