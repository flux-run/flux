use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;

#[derive(Subcommand)]
pub enum AgentCommands {
    /// List all agents
    List,
    /// Create a new agent
    Create {
        /// Name of the agent
        name: String,
        /// Model to use (e.g. gpt-4o)
        #[arg(long, value_name = "MODEL")]
        model: Option<String>,
    },
    /// Simulate an agent interaction
    Simulate {
        /// Name of the agent to simulate
        name: String,
    },
}

pub async fn execute(command: AgentCommands) -> anyhow::Result<()> {
    match command {
        AgentCommands::List => list_agents().await?,
        AgentCommands::Create { name, model } => {
            let client = ApiClient::new().await?;
            let mut body = serde_json::json!({ "name": name });
            if let Some(m) = model {
                body["model"] = Value::String(m);
            }
            let res = client.client
                .post(format!("{}/agents", client.base_url))
                .json(&body)
                .send()
                .await?;
            res.error_for_status()?;
            println!("Agent '{}' created.", name);
        }
        AgentCommands::Simulate { name } => {
            let client = ApiClient::new().await?;
            let res = client.client
                .post(format!("{}/agents/{}/simulate", client.base_url, name))
                .send()
                .await?;
            res.error_for_status()?;
            println!("Simulation started for agent '{}'.", name);
        }
    }
    Ok(())
}

async fn list_agents() -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let res = client.client
        .get(format!("{}/agents", client.base_url))
        .send()
        .await?;

    let json: Value = res.error_for_status()?.json().await?;
    let agents = json
        .get("data")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    println!("{:<30} {:<15} {}", "NAME", "MODEL", "STATUS");
    println!("{}", "-".repeat(60));
    for agent in agents {
        let name   = agent.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let model  = agent.get("model").and_then(|v| v.as_str()).unwrap_or("");
        let status = agent.get("status").and_then(|v| v.as_str()).unwrap_or("");
        println!("{:<30} {:<15} {}", name, model, status);
    }
    Ok(())
}
