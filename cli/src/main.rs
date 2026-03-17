use clap::{Parser, Subcommand};

mod auth;
mod build;
mod check;
mod config;
mod config_cmd;
mod exec;
mod grpc;
mod init;
mod logs;
mod process_state;
mod ps;
mod dev;
mod replay;
mod resume;
mod run;
mod runtime_process;
mod runtime_server;
mod serve;
mod server;
mod status;
mod tail;
mod trace;
mod why;
mod project;

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
    /// Save and verify runtime auth against a Flux server.
    Auth(auth::AuthArgs),
    /// Manage local Flux CLI config values.
    Config {
        #[command(subcommand)]
        command: config_cmd::ConfigCommand,
    },
    /// List recorded execution logs.
    Logs(logs::LogsArgs),
    /// Show managed Flux processes.
    Ps,
    /// Show overall Flux health status.
    Status,
    /// Run a one-off execution and record it.
    Exec(exec::ExecArgs),
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
    /// Analyse a JS/TS project and write flux.json for production use.
    Build(build::BuildArgs),
    /// Check compatibility with Flux's deterministic runtime contract.
    Check(check::CheckArgs),
    /// Start a development server with hot reload on file changes.
    Dev(dev::DevArgs),
    /// Run a JS/TS file as a plain script (no HTTP server).
    Run(run::RunArgs),
    /// Prepare a JS/TS entry file for runtime execution.
    Serve(serve::ServeArgs),
    /// Manage the Flux server process.
    Server {
        #[command(subcommand)]
        command: server::ServerCommand,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init(args) => init::execute(args).await?,
        Commands::Auth(args) => auth::execute(args).await?,
        Commands::Config { command } => config_cmd::execute(command)?,
        Commands::Logs(args) => logs::execute(args).await?,
        Commands::Ps => ps::execute().await?,
        Commands::Status => status::execute().await?,
        Commands::Exec(args) => exec::execute(args).await?,
        Commands::Trace(args) => trace::execute(args).await?,
        Commands::Replay(args) => replay::execute(args).await?,
        Commands::Resume(args) => resume::execute(args).await?,
        Commands::Why(args) => why::execute(args).await?,
        Commands::Tail(args) => tail::execute(args).await?,
        Commands::Build(args) => build::execute(args).await?,
        Commands::Check(args) => check::execute(args).await?,
        Commands::Dev(args) => dev::execute(args).await?,
        Commands::Run(args) => run::execute(args).await?,
        Commands::Serve(args) => serve::execute(args).await?,
        Commands::Server { command } => server::execute(command).await?,
    }

    Ok(())
}
