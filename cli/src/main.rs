use clap::{Parser, Subcommand};

mod auth;
mod config;
mod grpc;
mod serve;

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
    /// Prepare a JS/TS entry file for runtime execution.
    Serve(serve::ServeArgs),
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Auth(args) => auth::execute(args).await?,
        Commands::Serve(args) => serve::execute(args).await?,
    }

    Ok(())
}
