use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;
use std::fs;
use std::path::Path;

#[derive(Subcommand)]
pub enum FunctionCommands {
    /// Scaffold a new serverless function
    Create {
        name: String,
    },
    /// List deployed functions in the current project
    List,
}

pub async fn execute(command: FunctionCommands) -> anyhow::Result<()> {
    match command {
        FunctionCommands::Create { name } => {
            let dir_path = Path::new(&name);
            if dir_path.exists() {
                anyhow::bail!("Directory '{}' already exists.", name);
            }

            fs::create_dir_all(dir_path)?;

            let flux_json = serde_json::json!({
                "name": name,
                "runtime": "deno",
                "entry": "index.ts"
            });

            fs::write(
                dir_path.join("flux.json"),
                serde_json::to_string_pretty(&flux_json)?,
            )?;

            let index_ts = r#"export default async function(ctx: any) {
  return {
    message: "Hello from Fluxbase",
    payload: ctx.payload
  };
}
"#;
            fs::write(dir_path.join("index.ts"), index_ts)?;

            println!("Created function '{}' successfully.", name);
            println!("  cd {}", name);
            println!("  flux dev");
            println!("  flux deploy");
        }
        FunctionCommands::List => {
            let client = ApiClient::new().await?;
            let res = client.client
                .get(format!("{}/functions", client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let functions = json.get("data").and_then(|data| data.get("functions")).and_then(|v| v.as_array()).cloned().unwrap_or_default();
            
            println!("{:<40} {:<30} {:<10}", "ID", "NAME", "RUNTIME");
            for func in functions {
                let id = func.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let runtime = func.get("runtime").and_then(|v| v.as_str()).unwrap_or("");
                println!("{:<40} {:<30} {:<10}", id, name, runtime);
            }
        }
    }
    
    Ok(())
}
