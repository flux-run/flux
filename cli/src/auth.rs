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

    // Verify token
    let client = reqwest::Client::new();
    let res = client
        .get(format!("{}/auth/me", api_url))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    if !res.status().is_success() {
        anyhow::bail!("Authentication failed! Please check your API key.");
    }

    config.token = Some(token);

    config.save().await?;
    println!("Login successful!");

    Ok(())
}
