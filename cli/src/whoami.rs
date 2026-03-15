use crate::client::ApiClient;
use serde_json::Value;
use api_contract::routes as R;

pub async fn execute() -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let res = client.client
        .get(R::auth::ME.url(&client.base_url))
        .send()
        .await?;
    let json: Value = res.error_for_status()?.json().await?;
    println!("{}", serde_json::to_string_pretty(&json)?);
    Ok(())
}
