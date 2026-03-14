use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;

#[derive(Subcommand)]
pub enum ToolCommands {
    /// List available tools
    List,
}

pub async fn execute(command: ToolCommands) -> anyhow::Result<()> {
    match command {
        ToolCommands::List => list_tools().await?,
    }
    Ok(())
}

async fn list_tools() -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let res = client.client
        .get(format!("{}/tools", client.base_url))
        .send()
        .await?;

    let json: Value = res.error_for_status()?.json().await?;
    let tools = json
        .get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    println!("{:<30} {}", "NAME", "DESCRIPTION");
    println!("{}", "-".repeat(60));
    for tool in tools {
        let name = tool.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let desc = tool.get("description").and_then(|v| v.as_str()).unwrap_or("");
        println!("{:<30} {}", name, desc);
    }
    Ok(())
}
