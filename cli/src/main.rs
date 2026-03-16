use clap::{Parser, Subcommand};

mod bisect;
mod client;
mod config;
mod context;
mod incident;
mod logs;
mod server;
mod tail;
mod trace;
mod trace_diff;
mod why;

#[derive(Parser)]
#[command(name = "flux")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Flux CLI — debuggable JS runtime", long_about = None)]
struct Cli {
    /// Output raw JSON (machine-readable)
    #[arg(long, global = true)]
    json: bool,

    /// Disable coloured output
    #[arg(long, global = true)]
    no_color: bool,

    /// Auto-confirm prompts (non-interactive)
    #[arg(long, global = true)]
    yes: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Tail or stream platform logs
    Logs {
        source: Option<String>,
        resource: Option<String>,
        #[arg(short, long)]
        follow: bool,
        #[arg(long, default_value = "100", value_name = "N")]
        limit: u64,
    },

    /// Live request stream
    Tail {
        /// Filter to a specific function name
        function: Option<String>,
        /// Show only failed requests
        #[arg(long)]
        errors: bool,
        /// Show only requests slower than N ms
        #[arg(long, value_name = "MS")]
        slow: Option<u64>,
        /// Output raw JSON (one object per line)
        #[arg(long)]
        json: bool,
    },

    /// Show traces, or one full trace by request id
    Trace {
        /// Request ID to show (omit to list recent traces)
        request_id: Option<String>,
        #[arg(long, default_value = "500", value_name = "MS")]
        slow: u64,
        #[arg(long)]
        flame: bool,
        /// Filter / sort for `flux trace` list mode
        #[arg(long, value_name = "NAME")]
        function: Option<String>,
        /// Number of traces to list (default 20)
        #[arg(long, default_value = "20", value_name = "N")]
        limit: u64,
    },

    /// Root cause explanation for a request
    Why {
        /// Request ID to explain
        request_id: String,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },

    /// Re-run a recorded request in replay mode
    Replay {
        /// Request ID to replay
        request_id: String,
        /// Database name (default: "default")
        #[arg(long, default_value = "default")]
        database: String,
        /// Confirm replay without prompting
        #[arg(long)]
        yes: bool,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },

    /// Compare two executions (diff)
    Diff {
        /// Original request ID
        original_id: String,
        /// Replay/second request ID
        replay_id: String,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
        /// Only diff mutations for this table
        #[arg(long)]
        table: Option<String>,
    },

    /// Binary-search commit history to find first regression
    Bisect {
        /// Function name to bisect
        #[arg(long)]
        function: String,
        /// Known-good commit SHA
        #[arg(long)]
        good: String,
        /// Known-bad commit SHA
        #[arg(long)]
        bad: String,
        /// Failure-rate threshold to classify a commit as bad (0.0–1.0)
        #[arg(long, default_value = "0.05")]
        threshold: f64,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },

    /// Start the Flux runtime server for an existing JS entry file
    #[command(alias = "serve")]
    Server {
        /// Entry JavaScript file to run
        #[arg(value_name = "ENTRY", default_value = "index.js")]
        entry: String,
        /// Port to listen on
        #[arg(long, default_value = "8080", value_name = "PORT")]
        port: u16,
        /// Use release binary instead of debug
        #[arg(long)]
        release: bool,
        /// Disable coloured output
        #[arg(long)]
        no_color: bool,
        /// Override DATABASE_URL from the environment
        #[arg(long, value_name = "URL", env = "DATABASE_URL")]
        database_url: Option<String>,
    },

    /// Add or update a named connection to a Flux server instance
    Link {
        name: String,
        endpoint: String,
        #[arg(long, short, value_name = "KEY")]
        key: Option<String>,
    },
    /// Switch the active context
    Use { name: String },
    /// Show the current context and all configured contexts
    Context,
    /// Remove a named context
    Unlink { name: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.no_color {
        unsafe { std::env::set_var("NO_COLOR", "1"); }
    }

    match cli.command {
        Commands::Logs { source, resource, follow, limit } => {
            const SOURCES: &[&str] = &["function", "db", "event", "queue", "system"];
            let (resolved_source, resolved_resource) = match (source, resource) {
                (Some(s), r) if SOURCES.contains(&s.as_str()) => (Some(s), r),
                (Some(s), None) => (Some("function".to_string()), Some(s)),
                other => other,
            };
            if follow {
                logs::execute_follow(resolved_source, resolved_resource, limit).await?
            } else {
                logs::execute(resolved_source, resolved_resource, limit).await?
            }
        }
        Commands::Tail { function, errors, slow, json } => {
            tail::execute(function, errors, slow, json, false).await?
        }
        Commands::Trace { request_id, slow, flame, function, limit } => {
            match request_id {
                Some(id) => trace::execute(id, slow, flame).await?,
                None => trace::execute_list(limit, function, cli.json).await?,
            }
        }
        Commands::Why { request_id, json } => why::execute(request_id, json).await?,
        Commands::Replay { request_id, database, yes, json } => {
            incident::execute(incident::IncidentCommands::Replay {
                window: None,
                request_id: Some(request_id),
                from: None,
                to: None,
                database,
                yes,
                json,
            }).await?
        }
        Commands::Diff { original_id, replay_id, json, table } => {
            trace_diff::execute(original_id, replay_id, json, table).await?
        }
        Commands::Bisect { function, good, bad, threshold, json } => {
            bisect::execute(function, good, bad, threshold, json).await?
        }
        Commands::Server { entry, port, release, no_color, database_url } => {
            server::execute(entry, port, release, no_color, database_url).await?
        }
        Commands::Link { name, endpoint, key } => context::execute_link(name, endpoint, key)?,
        Commands::Use { name } => context::execute_use(name)?,
        Commands::Context => context::execute_context(None)?,
        Commands::Unlink { name } => context::execute_unlink(name)?,
    }

    Ok(())
}
