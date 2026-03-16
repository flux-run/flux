use clap::{Parser, Subcommand};

mod auth;
mod config;
mod config_cmd;
mod grpc;
mod logs;
mod serve;
mod server;
mod tail;
mod trace;

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
    /// Save and verify runtime auth against a Flux server.
    Auth(auth::AuthArgs),
    /// Manage local Flux CLI config values.
    Config {
        #[command(subcommand)]
        command: config_cmd::ConfigCommand,
    },
    /// List recorded execution logs.
    Logs(logs::LogsArgs),
    /// Show execution trace with checkpoints.
    Trace(trace::TraceArgs),
    /// Stream live execution events.
    Tail(tail::TailArgs),
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
        Commands::Auth(args) => auth::execute(args).await?,
        Commands::Config { command } => config_cmd::execute(command)?,
        Commands::Logs(args) => logs::execute(args).await?,
        Commands::Trace(args) => trace::execute(args).await?,
        Commands::Tail(args) => tail::execute(args).await?,
        Commands::Serve(args) => serve::execute(args).await?,
        Commands::Server { command } => server::execute(command).await?,
    }

    Ok(())
}
