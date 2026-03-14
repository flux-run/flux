use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;

#[derive(Subcommand)]
pub enum WorkflowCommands {
    /// List all workflows
    List,
    /// Create a new workflow
    Create {
        /// Name of the workflow
        name: String,
    },
    /// Run a workflow
    Run {
        /// Name of the workflow to run
        name: String,
        /// Optional JSON input payload
        #[arg(long, value_name = "JSON")]
        input: Option<String>,
    },
}

pub async fn execute(command: WorkflowCommands) -> anyhow::Result<()> {
    match command {
        WorkflowCommands::List => list_workflows().await?,
        WorkflowCommands::Create { name } => {
            let client = ApiClient::new().await?;
            let res = client.client
                .post(format!("{}/workflows", client.base_url))
                .json(&serde_json::json!({ "name": name }))
                .send()
                .await?;
            res.error_for_status()?;
            println!("Workflow '{}' created.", name);
        }
        WorkflowCommands::Run { name, input } => {
            let client = ApiClient::new().await?;
            let body = input
                .and_then(|s| serde_json::from_str::<Value>(&s).ok())
                .unwrap_or(Value::Object(Default::default()));
            let res = client.client
                .post(format!("{}/workflows/{}/run", client.base_url, name))
                .json(&body)
                .send()
                .await?;
            res.error_for_status()?;
            println!("Workflow '{}' triggered.", name);
        }
    }
    Ok(())
}

async fn list_workflows() -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let res = client.client
        .get(format!("{}/workflows", client.base_url))
        .send()
        .await?;

    let json: Value = res.error_for_status()?.json().await?;
    let workflows = json
        .get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    println!("{:<30} {:<12} {}", "NAME", "STATUS", "CREATED_AT");
    println!("{}", "-".repeat(65));
    for wf in workflows {
        let name       = wf.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let status     = wf.get("status").and_then(|v| v.as_str()).unwrap_or("");
        let created_at = wf.get("created_at").and_then(|v| v.as_str()).unwrap_or("");
        println!("{:<30} {:<12} {}", name, status, created_at);
    }
    Ok(())
}
