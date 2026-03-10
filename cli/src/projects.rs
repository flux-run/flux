use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;

#[derive(Subcommand)]
pub enum ProjectCommands {
    /// Create a new project under the active tenant
    Create {
        /// Name of the new project
        name: String,
        /// Also provision a default database for the project (default: true)
        #[arg(long, default_value = "true")]
        provision_db: bool,
    },
    /// List available projects
    List,
    /// Switch to a specific project
    Use {
        id: String,
    },
}

pub async fn execute(command: ProjectCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        ProjectCommands::Create { name, provision_db } => {
            let res = client.client
                .post(format!("{}/projects", client.base_url))
                .json(&serde_json::json!({ "name": name }))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);
            let project_id = data.get("project_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let slug = data.get("slug").and_then(|v| v.as_str()).unwrap_or("").to_string();
            println!("✓ Project created");
            println!("  id:   {}", project_id);
            println!("  slug: {}", slug);

            // Provision a default database for the project
            if provision_db {
                print!("  Provisioning default database... ");
                let db_res = client.client
                    .post(format!("{}/db/databases", client.base_url))
                    .header("X-Fluxbase-Project", &project_id)
                    .json(&serde_json::json!({ "name": "default" }))
                    .send()
                    .await;
                match db_res {
                    Ok(resp) if resp.status().is_success() => {
                        println!("done");
                        println!("✓ Default database ready");
                    }
                    Ok(resp) => {
                        let status = resp.status();
                        let body = resp.text().await.unwrap_or_default();
                        eprintln!("warning: {} — {}", status, body);
                    }
                    Err(e) => eprintln!("warning: {}", e),
                }
            }

            // Auto-select the new project
            let mut config = client.config;
            config.project_id = Some(project_id.clone());
            config.save().await?;
            println!("✓ Now using project: {}", project_id);
        }
        ProjectCommands::List => {
            let res = client.client
                .get(format!("{}/projects", client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let projects = json.get("data").and_then(|data| data.get("projects")).and_then(|v| v.as_array()).cloned().unwrap_or_default();
            
            println!("{:<40} {:<30}", "ID", "NAME");
            for project in projects {
                let id = project.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = project.get("name").and_then(|v| v.as_str()).unwrap_or("");
                println!("{:<40} {:<30}", id, name);
            }
        }
        ProjectCommands::Use { id } => {
            let mut config = client.config;
            config.project_id = Some(id.clone());
            config.save().await?;
            println!("Now using project: {}", id);
        }
    }
    
    Ok(())
}
