use clap::{Parser, Subcommand};

mod auth;
mod client;
mod config;
mod create;
mod db;
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
mod trace;

#[derive(Parser)]
#[command(name = "flux")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Fluxbase CLI — deploy backends in minutes", long_about = None)]
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
    /// Invoke a deployed function
    Invoke {
        /// Function name to invoke
        name: String,
        /// JSON payload to pass to the function (e.g. '{"a":1}')
        #[arg(long, value_name = "JSON")]
        payload: Option<String>,
        /// Route through the gateway (auth + rate-limiting) instead of calling runtime directly
        #[arg(long)]
        gateway: bool,
        #[arg(long, hide = true)]
        tenant: Option<String>,
    },
    /// Tail or stream platform logs (functions, databases, workflows, …)
    ///
    /// Examples:
    ///   flux logs                     — all logs in project
    ///   flux logs function echo       — function/echo logs
    ///   flux logs db users            — db/users logs
    ///   flux logs workflow wf_123     — workflow logs
    ///   flux logs echo                — backward compat → function/echo
    Logs {
        /// Source subsystem: function | db | workflow | event | queue | system
        /// (omit to show all; single non-subsystem word treated as function name)
        source: Option<String>,
        /// Resource name within the source (function name, db name, etc.)
        resource: Option<String>,
        /// Stream live — poll for new lines every 1.5s (Ctrl+C to stop)
        #[arg(short, long)]
        follow: bool,
        /// Number of recent log lines to fetch (default 100)
        #[arg(long, default_value = "100", value_name = "N")]
        limit: u64,
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
    /// Show the full cross-service request trace for a given request ID
    ///
    /// Example:
    ///   flux trace abc123-def456
    Trace {
        /// Request ID to look up (returned as x-request-id in API responses)
        request_id: String,
        /// Milliseconds threshold above which a span delta is highlighted as slow (default 500)
        #[arg(long, default_value = "500", value_name = "MS")]
        slow: u64,
        /// Render a terminal flame graph (waterfall timeline) instead of the span table
        #[arg(long)]
        flame: bool,
    },
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
        /// Override Runtime URL for this project (e.g. http://localhost:8083)
        #[arg(long, value_name = "URL")]
        runtime_url: Option<String>,
    },
    /// Create a new project from an official template
    ///
    /// Examples:
    ///   flux create my-app
    ///   flux create my-app --template todo-api
    ///   flux create my-app --template webhook-worker
    ///   flux create my-app --template ai-backend
    Create {
        /// Project directory name to create
        name: String,
        /// Template to scaffold (todo-api | webhook-worker | ai-backend)
        /// Omit to choose interactively.
        #[arg(long, short, value_name = "TEMPLATE")]
        template: Option<String>,
    },
    /// Database operations (create, list, manage tables)
    Db {
        #[command(subcommand)]
        command: db::DbCommands,
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
        Commands::Invoke { name, tenant, payload, gateway } => invoke::execute(&name, tenant, payload, gateway).await?,
        Commands::Logs { source, resource, follow, limit } => {
            // Backward compat: single positional that isn't a known subsystem
            // is treated as a resource_id under "function".
            const SOURCES: &[&str] = &["function", "db", "workflow", "event", "queue", "system"];
            let (resolved_source, resolved_resource) = match (source, resource) {
                (Some(s), r) if SOURCES.contains(&s.as_str()) => (Some(s), r),
                (Some(s), None)  => (Some("function".to_string()), Some(s)),
                other => other,
            };
            if follow {
                logs::execute_follow(resolved_source, resolved_resource, limit).await?
            } else {
                logs::execute(resolved_source, resolved_resource, limit).await?
            }
        }
        Commands::Deployments { command } => deployments::execute_deployments(command).await?,
        Commands::Rollback { name, version } => deployments::execute_rollback(&name, version).await?,
        Commands::Pull { output } => sdk::execute_pull(output).await?,
        Commands::Watch { output, interval } => sdk::execute_watch(output, interval).await?,
        Commands::Status { sdk } => sdk::execute_status(sdk).await?,
        Commands::Doctor => doctor::execute().await?,
        Commands::Trace { request_id, slow, flame } => trace::execute(request_id, slow, flame).await?,
        Commands::Init { project, output, interval, api_url, gateway_url, runtime_url } => init::execute(project, output, interval, api_url, gateway_url, runtime_url).await?,
        Commands::Create { name, template } => create::execute(name, template).await?,
        Commands::Db { command } => db::execute(command).await?,
        Commands::Stack { command } => match command {
            StackCommand::Up   { build, foreground }  => stack::execute_up(build, !foreground).await?,
            StackCommand::Down { volumes }             => stack::execute_down(volumes).await?,
            StackCommand::Ps                          => stack::execute_ps().await?,
            StackCommand::Logs { service, tail }       => stack::execute_logs(service, tail).await?,
        },
    }

    Ok(())
}
