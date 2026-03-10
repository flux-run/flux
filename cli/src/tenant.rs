use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;

#[derive(Subcommand)]
pub enum TenantCommands {
    /// Create a new tenant (organization)
    Create {
        /// Name of the new organization
        name: String,
    },
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
        TenantCommands::Create { name } => {
            let res = client.client
                .post(format!("{}/tenants", client.base_url))
                .json(&serde_json::json!({ "name": name }))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);
            let tenant_id = data.get("tenant_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let slug = data.get("slug").and_then(|v| v.as_str()).unwrap_or("").to_string();
            println!("✓ Tenant created");
            println!("  id:   {}", tenant_id);
            println!("  slug: {}", slug);

            // Auto-select the new tenant
            let mut config = client.config;
            config.tenant_id = Some(tenant_id.clone());
            config.project_id = None; // clear stale project from another org
            config.save().await?;
            println!("✓ Now using tenant: {}", tenant_id);
        }
        TenantCommands::List => {
            let res = client.client
                .get(format!("{}/tenants", client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let tenants = json.get("data").and_then(|d| d.get("tenants")).and_then(|v| v.as_array()).cloned().unwrap_or_default();
            
            println!("{:<40} {:<30} {:<10}", "ID", "NAME", "ROLE");
            for tenant in tenants {
                let id = tenant.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = tenant.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let role = tenant.get("role").and_then(|v| v.as_str()).unwrap_or("");
                println!("{:<40} {:<30} {:<10}", id, name, role);
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
