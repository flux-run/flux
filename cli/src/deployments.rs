use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;

#[derive(Subcommand)]
pub enum DeploymentCommands {
    /// List deployments for a function
    List {
        name: String,
    },
}

pub async fn execute_deployments(command: DeploymentCommands) -> anyhow::Result<()> {
    match command {
        DeploymentCommands::List { name } => {
            let client = ApiClient::new().await?;
            let res = client.client
                .get(format!("{}/functions/{}/deployments", client.base_url, name))
                .send()
                .await?;
            
            let json: Value = res.error_for_status()?.json().await?;
            let deployments = json
                .get("data")
                .and_then(|d| d.get("deployments"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            println!("{:<36} {:<10} {:<15} {}", "ID", "VERSION", "STATUS", "CREATED_AT");
            println!("{}", "-".repeat(85));
            for dep in deployments {
                let id = dep.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let version = dep.get("version").and_then(|v| v.as_i64()).unwrap_or(0);
                let is_active = dep.get("is_active").and_then(|v| v.as_bool()).unwrap_or(false);
                let status = dep.get("status").and_then(|v| v.as_str()).unwrap_or("");
                let created = dep.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
                
                let active_marker = if is_active { "(Active)" } else { "" };
                let version_str = format!("v{} {}", version, active_marker);
                
                println!("{:<36} {:<10} {:<15} {}", id, version_str, status, created);
            }
        }
    }
    Ok(())
}

pub async fn execute_rollback(name: &str, version: i32) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let res = client.client
        .post(format!("{}/functions/{}/deployments/{}/activate", client.base_url, name, version))
        .send()
        .await?;
    
    res.error_for_status()?;
    
    println!("✅ Rolled back function '{}' to version v{}", name, version);
    Ok(())
}
