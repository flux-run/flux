use crate::config::Config;

pub async fn execute() -> anyhow::Result<()> {
    println!("Welcome to Fluxbase CLI!");

    // Prompt securely for API Key without echoing
    let token = rpassword::prompt_password("Enter API Key: ")?;
    let token = token.trim().to_string();

    if !token.starts_with("flux_") {
        anyhow::bail!("Invalid API Key format. Keys must begin with 'flux_'");
    }

    let mut config = Config::load().await;
    let api_url = config.api_url.clone();

    // Verify token and get context from /auth/me
    let client = reqwest::Client::new();
    let res = client
        .get(format!("{}/auth/me", api_url))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !res.status().is_success() {
        anyhow::bail!("Authentication failed! Please check your API key.");
    }

    let me: serde_json::Value = res.json().await.unwrap_or_default();
    let data = me.get("data").unwrap_or(&me);

    config.token = Some(token);

    // API key logins return tenant_id/project_id directly in the response
    if let Some(tid) = data.get("tenant_id").and_then(|v| v.as_str()) {
        config.tenant_id = Some(tid.to_string());
        println!("Auto-selected tenant: {}", tid);
    }
    if let Some(pid) = data.get("project_id").and_then(|v| v.as_str()) {
        config.project_id = Some(pid.to_string());
        println!("Auto-selected project: {}", pid);
    }

    config.save().await?;
    println!("Login successful!");

    Ok(())
}
