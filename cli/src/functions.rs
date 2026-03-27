use crate::config;
use crate::grpc;
use anyhow::Result;
use clap::{Args, Subcommand};
use tabled::{Table, Tabled};
// use shared::pb;

#[derive(Args)]
pub struct FunctionsArgs {
    #[command(subcommand)]
    pub command: FunctionsCommand,
}

#[derive(Subcommand)]
pub enum FunctionsCommand {
    /// List all functions in a project
    List {
        #[arg(long, short)]
        project_id: Option<String>,
    },
    /// Delete a function by ID
    Delete {
        #[arg(long, short)]
        function_id: String,
    },
}

#[derive(Tabled)]
struct FunctionRow {
    id: String,
    name: String,
    created_at: String,
}

pub async fn execute(args: FunctionsArgs) -> Result<()> {
    match args.command {
        FunctionsCommand::List { project_id } => {
            let auth = config::resolve_optional_auth(None, None)?;
            let pid = project_id.or(auth.project_id).expect("No project_id found. Please specify --project-id or use 'flux login --project-id'.");

            println!("🔍 Fetching functions for project {}...", pid);
            let functions = grpc::list_functions(&auth.url, &auth.token, &pid).await?;

            if functions.is_empty() {
                println!("No functions found for this project.");
                return Ok(());
            }

            let rows: Vec<FunctionRow> = functions
                .into_iter()
                .map(|f| FunctionRow {
                    id: f.id,
                    name: f.name,
                    created_at: f.created_at,
                })
                .collect();

            println!("{}", Table::new(rows));
        }
        FunctionsCommand::Delete { function_id } => {
            let auth = config::resolve_optional_auth(None, None)?;
            println!("🗑️  Deleting function {}...", function_id);
            grpc::delete_function(&auth.url, &auth.token, &function_id).await?;
            println!("✅ Function deleted.");
        }
    }
    Ok(())
}

#[derive(Args)]
pub struct EnvArgs {
    #[command(subcommand)]
    pub command: EnvCommand,
}

#[derive(Subcommand)]
pub enum EnvCommand {
    /// List all environment variables for a project
    List {
        #[arg(long, short)]
        project_id: Option<String>,
    },
    /// Set an environment variable
    Set {
        #[arg(long, short)]
        project_id: Option<String>,
        key: String,
        value: String,
    },
    /// Delete an environment variable
    Delete {
        #[arg(long, short)]
        project_id: Option<String>,
        key: String,
    },
}

#[derive(Tabled)]
struct EnvVarRow {
    key: String,
    value: String,
    updated_at: String,
}

pub async fn execute_env(args: EnvArgs) -> Result<()> {
    match args.command {
        EnvCommand::List { project_id } => {
            let auth = config::resolve_optional_auth(None, None)?;
            let pid = project_id.or(auth.project_id).expect("No project_id found. Please specify --project-id or use 'flux login --project-id'.");

            println!("🔑 Fetching environment variables for project {}...", pid);
            let vars = grpc::list_env_vars(&auth.url, &auth.token, &pid).await?;

            if vars.is_empty() {
                println!("No environment variables found for this project.");
                return Ok(());
            }

            let rows: Vec<EnvVarRow> = vars
                .into_iter()
                .map(|v| EnvVarRow {
                    key: v.key,
                    value: v.value,
                    updated_at: v.updated_at,
                })
                .collect();

            println!("{}", Table::new(rows));
        }
        EnvCommand::Set {
            project_id,
            key,
            value,
        } => {
            let auth = config::resolve_optional_auth(None, None)?;
            let pid = project_id.or(auth.project_id).expect("No project_id found. Please specify --project-id or use 'flux login --project-id'.");

            println!(
                "🚀 Setting environment variable {}={}...",
                key,
                key.chars().map(|_| '*').collect::<String>()
            );
            grpc::set_env_var(&auth.url, &auth.token, &pid, &key, &value).await?;
            println!("✅ Environment variable set successfully.");
        }
        EnvCommand::Delete { project_id, key } => {
            let auth = config::resolve_optional_auth(None, None)?;
            let pid = project_id.or(auth.project_id).expect("No project_id found. Please specify --project-id or use 'flux login --project-id'.");

            println!("🗑️  Deleting environment variable {}...", key);
            grpc::delete_env_var(&auth.url, &auth.token, &pid, &key).await?;
            println!("✅ Environment variable deleted successfully.");
        }
    }
    Ok(())
}
