use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;

#[derive(Subcommand)]
pub enum TenantCommands {
    /// List available tenants
    List,
    /// Switch to a specific tenant
    Use {
        id: String,
    },
}

pub async fn execute(command: TenantCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        TenantCommands::List => {
            let res = client.client
                .get(format!("{}/tenants", client.base_url))
                .send()
                .await?;
            let tenants: Vec<Value> = res.error_for_status()?.json().await?;
            
            println!("{:<40} {:<30}", "ID", "NAME");
            for tenant in tenants {
                let id = tenant.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = tenant.get("name").and_then(|v| v.as_str()).unwrap_or("");
                println!("{:<40} {:<30}", id, name);
            }
        }
        TenantCommands::Use { id } => {
            let mut config = client.config;
            config.tenant_id = Some(id.clone());
            config.save().await?;
            println!("Now using tenant: {}", id);
        }
    }
    
    Ok(())
}
