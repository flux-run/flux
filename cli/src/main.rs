use clap::{Parser, Subcommand};

mod add;
mod bin_resolution;
mod build;
mod check;
mod config;
mod config_cmd;
mod dev;
mod events;
mod exec;
mod grpc;
mod init;
mod login;
mod logs;
mod process_state;
mod project;
mod ps;
mod replay;
mod resume;
mod run;
mod runtime_process;
mod runtime_runner;
mod server;
mod status;
mod start;
mod tui;
mod tail;
mod trace;
mod why;
mod deployments;

#[derive(Parser)]
#[command(name = "flux")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Flux CLI — auth and JS runtime entry handling", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a Flux project or migrate auth setup with explicit mode flags.
    Init(init::InitArgs),
    /// Add a package to the project (npm: or https:).
    Add(add::AddArgs),
    /// Log in to Flux Cloud or a self-hosted Flux server.
    Login(login::LoginArgs),
    /// Log out and clear local credentials.
    Logout,
    /// Manage local Flux CLI config values.
    Config {
        #[command(subcommand)]
        command: config_cmd::ConfigCommand,
    },
    /// List execution logs.
    Logs(logs::LogsArgs),
    /// Show managed Flux processes.
    Ps,
    /// Show overall Flux health status.
    Status,
    /// Show execution trace with checkpoints.
    Trace(trace::TraceArgs),
    /// Replay an execution using recorded checkpoints.
    Replay(replay::ReplayArgs),
    /// Resume an execution from a checkpoint boundary.
    Resume(resume::ResumeArgs),
    /// Explain why an execution failed or was slow.
    Why(why::WhyArgs),
    /// Stream live execution events.
    Tail(tail::TailArgs),
    /// Manually trigger a test notification for tail.
    PingTail {
        #[arg(long)]
        project_id: Option<String>,
    },
    /// Analyse a JS/TS project and write flux.json for production use.
    Build(build::BuildArgs),
    /// Check compatibility with Flux's deterministic runtime contract.
    Check(check::CheckArgs),
    /// Start a development server with hot reload on file changes.
    Dev(dev::DevArgs),
    /// Run a JS/TS file as a plain script or a long-running server.
    Run(run::RunArgs),
    /// Start the current Flux project using the latest build artifact.
    Start(start::StartArgs),
    /// Manage the Flux server process.
    Server {
        #[command(subcommand)]
        command: server::ServerCommand,
    },
    /// List the project's build history and deployments.
    Deployments(deployments::DeploymentsArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init(args) => init::execute(args).await?,
        Commands::Add(args) => add::execute(args).await?,
        Commands::Login(args) => login::execute(args).await?,
        Commands::Logout => {
            let mut config = config::CliConfig::load()?;
            config.token = None;
            config.save()?;
            println!("logged out");
        }
        Commands::Config { command } => config_cmd::execute(command)?,
        Commands::Logs(args) => logs::execute(args).await?,
        Commands::Ps => ps::execute().await?,
        Commands::Status => status::execute().await?,
        Commands::Trace(args) => trace::execute(args).await?,
        Commands::Replay(args) => replay::execute(args).await?,
        Commands::Resume(args) => resume::execute(args).await?,
        Commands::Why(args) => why::execute(args).await?,
        Commands::Tail(args) => tail::execute(args).await?,
        Commands::PingTail { project_id } => {
            let auth = config::resolve_optional_auth(None, None)?;
            grpc::ping_tail(&auth.url, &auth.token, project_id).await?;
            println!("ping-tail sent");
        }
        Commands::Build(args) => build::execute(args).await?,
        Commands::Check(args) => check::execute(args).await?,
        Commands::Dev(args) => dev::execute(args).await?,
        Commands::Run(args) => run::execute(args).await?,
        Commands::Start(args) => start::execute(args).await?,
        Commands::Server { command } => server::execute(command).await?,
        Commands::Deployments(args) => deployments::execute(args).await?,
    }

    Ok(())
}
