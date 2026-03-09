use clap::{Parser, Subcommand};

mod auth;
mod client;
mod config;
mod deploy;
mod dev;
mod doctor;
mod functions;
mod init;
mod invoke;
mod logs;
mod projects;
mod sdk;
mod secrets;
mod tenant;
mod deployments;

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
        #[arg(long)]
        tenant: Option<String>,
    },
    /// Fetch function logs
    Logs {
        name: String,
    },
    /// Deployment operations
    Deployments {
        #[command(subcommand)]
        command: deployments::DeploymentCommands,
    },
    /// Rollback function to a specific version
    Rollback {
        name: String,
        #[arg(long)]
        version: i32,
    },
    /// Pull the TypeScript SDK for the current project
    Pull {
        /// Output file path (default: fluxbase.generated.ts)
        #[arg(long, short, value_name = "FILE")]
        output: Option<String>,
    },
    /// Watch schema for changes and auto-regenerate the SDK
    Watch {
        /// Output file path (default: fluxbase.generated.ts)
        #[arg(long, short, value_name = "FILE")]
        output: Option<String>,
        /// Polling interval in seconds (default: 5)
        #[arg(long, default_value = "5", value_name = "SECS")]
        interval: u64,
    },
    /// Show local vs remote schema version status
    Status {
        /// Path to the generated SDK file (default: fluxbase.generated.ts)
        #[arg(long, short, value_name = "FILE")]
        sdk: Option<String>,
    },
    /// Diagnose environment, connectivity, and SDK sync
    Doctor,
    /// Initialise .fluxbase/config.json for this project
    Init {
        /// Fluxbase project ID to store in the config
        #[arg(long, value_name = "PROJECT_ID")]
        project: Option<String>,
        /// Default SDK output path
        #[arg(long, value_name = "FILE")]
        output: Option<String>,
        /// Default watch interval in seconds
        #[arg(long, value_name = "SECS")]
        interval: Option<u64>,
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
        Commands::Invoke { name, tenant } => invoke::execute(&name, tenant).await?,
        Commands::Logs { name } => logs::execute(&name).await?,
        Commands::Deployments { command } => deployments::execute_deployments(command).await?,
        Commands::Rollback { name, version } => deployments::execute_rollback(&name, version).await?,
        Commands::Pull { output } => sdk::execute_pull(output).await?,
        Commands::Watch { output, interval } => sdk::execute_watch(output, interval).await?,
        Commands::Status { sdk } => sdk::execute_status(sdk).await?,
        Commands::Doctor => doctor::execute().await?,
        Commands::Init { project, output, interval } => init::execute(project, output, interval).await?,
    }

    Ok(())
}
