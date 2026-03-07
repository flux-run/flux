use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;

#[derive(Subcommand)]
pub enum SecretsCommands {
    /// List secrets for the current project
    List,
    /// Set a secret
    Set {
        key: String,
        value: String,
    },
    /// Delete a secret
    Delete {
        key: String,
    },
}

pub async fn execute(command: SecretsCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        SecretsCommands::List => {
            let res = client.client
                .get(format!("{}/secrets", client.base_url))
                .send()
                .await?;
            let secrets: Vec<Value> = res.error_for_status()?.json().await?;
            
            println!("{:<30} {:<30} {:<10}", "KEY", "UPDATED_AT", "VERSION");
            for secret in secrets {
                let key = secret.get("key").and_then(|v| v.as_str()).unwrap_or("");
                let updated = secret.get("updated_at").and_then(|v| v.as_str()).unwrap_or("");
                let version = secret.get("version").and_then(|v| v.as_f64()).unwrap_or(1.0);
                println!("{:<30} {:<30} {:<10}", key, updated, version);
            }
        }
        SecretsCommands::Set { key, value } => {
            let payload = serde_json::json!({
                "key": key,
                "value": value,
            });
            let res = client.client
                .post(format!("{}/secrets", client.base_url))
                .json(&payload)
                .send()
                .await?;
            res.error_for_status()?;
            println!("Secret '{}' set successfully.", key);
        }
        SecretsCommands::Delete { key } => {
            let res = client.client
                .delete(format!("{}/secrets/{}", client.base_url, key))
                .send()
                .await?;
            res.error_for_status()?;
            println!("Secret '{}' deleted successfully.", key);
        }
    }
    
    Ok(())
}
