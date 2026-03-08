use clap::Subcommand;
use crate::client::ApiClient;
use serde_json::Value;
use std::fs;
use std::path::Path;

#[derive(Subcommand)]
pub enum FunctionCommands {
    /// Scaffold a new TypeScript serverless function
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

            // flux.json — TypeScript-first, deno runtime
            let flux_json = serde_json::json!({
                "runtime": "deno",
                "entry": "index.ts"
            });
            fs::write(
                dir_path.join("flux.json"),
                serde_json::to_string_pretty(&flux_json)?,
            )?;

            // package.json — resolves @fluxbase/functions from the monorepo workspace
            let pkg_json = serde_json::json!({
                "name": name,
                "version": "0.1.0",
                "private": true,
                "type": "module",
                "scripts": {
                    "build": "esbuild index.ts --bundle --platform=neutral --format=iife --global-name=__fluxbase_fn --outdir=dist",
                    "dev": "node --watch dist/bundle.js"
                },
                "dependencies": {
                    "@fluxbase/functions": "*",
                    "zod": "^3.23.0"
                }
            });
            fs::write(
                dir_path.join("package.json"),
                serde_json::to_string_pretty(&pkg_json)?,
            )?;

            // index.ts — uses defineFunction with Zod schema validation
            let index_ts = format!(r#"import {{ defineFunction }} from "@fluxbase/functions"
import {{ z }} from "zod"

const Input = z.object({{
  name: z.string(),
}})

const Output = z.object({{
  message: z.string(),
}})

export default defineFunction({{
  name: "{}",
  description: "A simple hello-world function",

  input: Input,
  output: Output,

  handler: async ({{ input, ctx }}) => {{
    ctx.log("Executing {} handler")

    return {{
      message: `Hello ${{input.name}}`,
    }}
  }},
}})
"#, name, name);
            fs::write(dir_path.join("index.ts"), index_ts)?;

            println!("✅ Created function '{}'", name);
            println!("");
            println!("  cd {}", name);
            println!("  npm install          # install @fluxbase/functions + zod");
            println!("  flux deploy          # bundle & deploy");
            println!("  flux invoke {}  # test it", name);
        }

        FunctionCommands::List => {
            let client = ApiClient::new().await?;
            let res = client.client
                .get(format!("{}/functions", client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let functions = json
                .get("data")
                .and_then(|d| d.get("functions"))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            println!("{:<40} {:<25} {:<10} DESCRIPTION", "ID", "NAME", "RUNTIME");
            println!("{}", "-".repeat(100));
            for func in functions {
                let id = func.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let name = func.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let runtime = func.get("runtime").and_then(|v| v.as_str()).unwrap_or("");
                let desc = func.get("description").and_then(|v| v.as_str()).unwrap_or("-");
                println!("{:<40} {:<25} {:<10} {}", id, name, runtime, desc);
            }
        }
    }

    Ok(())
}
