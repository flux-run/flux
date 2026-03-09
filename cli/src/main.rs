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
mod stack;
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
    /// Deploy to Fluxbase.
    /// In a function directory (has flux.json): deploys that single function.
    /// At the project root: discovers and deploys all function sub-directories.
    Deploy {
        /// Override function name (single-function mode only)
        #[arg(long)]
        name: Option<String>,
        /// Override runtime (single-function mode only)
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
        /// Override API URL for this project (e.g. http://localhost:8080)
        #[arg(long, value_name = "URL")]
        api_url: Option<String>,
        /// Override Gateway URL for this project (e.g. http://localhost:8081)
        #[arg(long, value_name = "URL")]
        gateway_url: Option<String>,
    },
    /// Manage the local Fluxbase development stack (all services via Docker)
    Stack {
        #[command(subcommand)]
        command: StackCommand,
    },
}

#[derive(Subcommand)]
enum StackCommand {
    /// Build and start all services (detached by default)
    Up {
        /// Force rebuild of Docker images before starting
        #[arg(long)]
        build: bool,
        /// Run in foreground (default is detached / -d)
        #[arg(long)]
        foreground: bool,
    },
    /// Stop and remove containers
    Down {
        /// Also remove the postgres data volume (destroys all local data)
        #[arg(short, long)]
        volumes: bool,
    },
    /// List running services and their exposed ports
    Ps,
    /// Tail logs for one or all services
    Logs {
        /// Service name to tail (omit to tail all)
        service: Option<String>,
        /// Number of recent log lines to show
        #[arg(long, default_value = "100")]
        tail: u32,
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
        Commands::Init { project, output, interval, api_url, gateway_url } => init::execute(project, output, interval, api_url, gateway_url).await?,
        Commands::Stack { command } => match command {
            StackCommand::Up   { build, foreground }  => stack::execute_up(build, !foreground).await?,
            StackCommand::Down { volumes }             => stack::execute_down(volumes).await?,
            StackCommand::Ps                          => stack::execute_ps().await?,
            StackCommand::Logs { service, tail }       => stack::execute_logs(service, tail).await?,
        },
    }

    Ok(())
}
