//! `flux whoami` — print the currently authenticated user and active context.

use colored::Colorize;

use crate::config::Config;

pub async fn execute() -> anyhow::Result<()> {
    let config = Config::load().await;

    let token = config
        .token
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Not authenticated. Run `flux login` first."))?;

    // Fetch user info
    let http = reqwest::Client::new();
    let res = http
        .get(format!("{}/auth/me", config.api_url))
        .header("Authorization", format!("Bearer {}", token))
        .send()
        .await?;

    let email = if res.status().is_success() {
        let json: serde_json::Value = res.json().await.unwrap_or_default();
        let data = json.get("data").unwrap_or(&json);
        data.get("email")
            .and_then(|v| v.as_str())
            .unwrap_or("(unknown)")
            .to_string()
    } else {
        "(token may be expired)".to_string()
    };

    println!();
    let label_w = 10;
    println!(
        "  {}  {}",
        format!("{:<label_w$}", "user:").bold(),
        email.cyan()
    );

    if let Some(slug) = &config.tenant_slug {
        let id_part = config
            .tenant_id
            .as_deref()
            .map(|id| format!("({})", &id[..id.len().min(8)]))
            .unwrap_or_default();
        println!(
            "  {}  {}  {}",
            format!("{:<label_w$}", "tenant:").bold(),
            slug.cyan(),
            id_part.dimmed()
        );
    } else if let Some(tid) = &config.tenant_id {
        println!(
            "  {}  {}",
            format!("{:<label_w$}", "tenant:").bold(),
            tid.cyan()
        );
    } else {
        println!(
            "  {}  {}",
            format!("{:<label_w$}", "tenant:").bold(),
            "(not set — run `flux tenant use <id>`)".yellow()
        );
    }

    if let Some(pid) = &config.project_id {
        println!(
            "  {}  {}",
            format!("{:<label_w$}", "project:").bold(),
            pid.cyan()
        );
    } else {
        println!(
            "  {}  {}",
            format!("{:<label_w$}", "project:").bold(),
            "(not set — run `flux project use <id>`)".yellow()
        );
    }

    // Mask token for display: show first 12 chars
    let token_preview = if token.len() > 12 {
        format!("{}…", &token[..12])
    } else {
        token.to_string()
    };
    println!(
        "  {}  {}",
        format!("{:<label_w$}", "token:").bold(),
        token_preview.dimmed()
    );

    println!();
    Ok(())
}
