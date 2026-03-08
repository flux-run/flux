use clap::{Parser, Subcommand};

mod auth;
mod client;
mod config;
mod deploy;
mod dev;
mod functions;
mod invoke;
mod logs;
mod projects;
mod secrets;
mod tenant;

#[derive(Parser)]
#[command(name = "flux")]
#[command(about = "Fluxbase CLI", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate with Fluxbase
    Login,
    /// Tenant operations
    Tenant {
        #[command(subcommand)]
        command: tenant::TenantCommands,
    },
    /// Project operations
    Project {
        #[command(subcommand)]
        command: projects::ProjectCommands,
    },
    /// Function operations
    Function {
        #[command(subcommand)]
        command: functions::FunctionCommands,
    },
    /// Secrets operations
    Secrets {
        #[command(subcommand)]
        command: secrets::SecretsCommands,
    },
    /// Run function locally
    Dev,
    /// Deploy function from current directory (requires flux.json)
    Deploy {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        runtime: Option<String>,
    },
    /// Invoke function
    Invoke {
        name: String,
    },
    /// Fetch function logs
    Logs {
        name: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Login => auth::execute().await?,
        Commands::Tenant { command } => tenant::execute(command).await?,
        Commands::Project { command } => projects::execute(command).await?,
        Commands::Function { command } => functions::execute(command).await?,
        Commands::Secrets { command } => secrets::execute(command).await?,
        Commands::Dev => dev::execute().await?,
        Commands::Deploy { name, runtime } => deploy::execute(name, runtime).await?,
        Commands::Invoke { name } => invoke::execute(&name).await?,
        Commands::Logs { name } => logs::execute(&name).await?,
    }

    Ok(())
}
