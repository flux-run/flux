use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;

#[derive(Subcommand)]
pub enum ProjectCommands {
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
        ProjectCommands::List => {
            let res = client.client
                .get(format!("{}/projects", client.base_url))
                .send()
                .await?;
            let projects: Vec<Value> = res.error_for_status()?.json().await?;
            
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
