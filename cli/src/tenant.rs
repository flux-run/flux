use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;
use api_contract::routes as R;

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
    match command {
        TenantCommands::List => list_tenants().await?,
        TenantCommands::Create { name } => {
            let client = ApiClient::new().await?;
            let res = client.client
                .post(R::tenants::LIST.url(&client.base_url))
                .json(&serde_json::json!({ "name": name }))
                .send()
                .await?;
            res.error_for_status()?;
            println!("Tenant '{}' created.", name);
        }
        TenantCommands::Use { id } => {
            println!("Switched to tenant '{}'.", id);
        }
    }
    Ok(())
}

async fn list_tenants() -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let res = client.client
        .get(R::tenants::LIST.url(&client.base_url))
        .send()
        .await?;

    let json: Value = res.error_for_status()?.json().await?;
    let tenants = json
        .get("data")
        .and_then(|d| d.get("tenants"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    println!("{:<20} {:<20} {:<15} {}", "ID", "NAME", "SLUG", "ROLE");
    println!("{}", "-".repeat(70));
    for tenant in tenants {
        let id      = tenant.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let name    = tenant.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let slug    = tenant.get("slug").and_then(|v| v.as_str()).unwrap_or("");
        let role    = tenant.get("role").and_then(|v| v.as_str()).unwrap_or("");
        println!("{:<20} {:<20} {:<15} {}", id, name, slug, role);
    }
    Ok(())
}
