use crate::client::ApiClient;


pub async fn execute(name: &str, tenant_slug: Option<String>) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    // Default to the provided public runtime URL
    let runtime_url = std::env::var("FLUXBASE_RUNTIME_URL")
        .unwrap_or_else(|_| "http://localhost:8081".to_string());

    let exec_url = format!("{}/execute", runtime_url);

    let tenant_id = client.config.tenant_id.clone().unwrap_or_default();
    
    // Fallback to auto-selected tenant slug if not provided via args
    let slug = tenant_slug.or_else(|| client.config.tenant_slug.clone())
        .unwrap_or_else(|| tenant_id.clone());

    let payload = serde_json::json!({
        "function_id": name,
        "tenant_id": tenant_id,
        "payload": { "invoked_by": "flux-cli" }
    });

    println!("Invoking {}...", name);

    let res = client.client
        .post(&exec_url)
        .header("Authorization", format!("Bearer {}", client.config.token.unwrap_or_default()))
        .header("X-Tenant-Id", &tenant_id)
        .header("X-Tenant-Slug", &slug)
        .json(&payload)
        .send()
        .await?;

    let status = res.status();
    let body = res.text().await?;

    if status.is_success() {
        println!("Success:\n{}", body);
    } else {
        println!("Error ({}):\n{}", status, body);
    }

    Ok(())
}
