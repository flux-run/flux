use clap::{Parser, Subcommand};

mod agent;
mod api_key;
mod auth;
mod bisect;
mod client;
mod config;
mod config_cmd;
mod create;
mod db;
mod debug;
mod deploy;
mod deployments;
mod dev;
mod doctor;
mod env_cmd;
mod errors;
mod event;
mod explain;
mod functions;
mod gateway;
mod incident;
mod init;
mod invoke;
mod logs;
mod monitor;
mod open;
mod projects;
mod queue;
mod schedule;
mod sdk;
mod secrets;
mod stack;
mod state;
mod tail;
mod tenant;
mod tool;
mod trace;
mod trace_debug;
mod trace_diff;
mod upgrade;
mod version_cmd;
mod whoami;
mod why;
mod workflow;

#[derive(Parser)]
#[command(name = "flux")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Fluxbase CLI — deploy backends in minutes", long_about = None)]
struct Cli {
    /// Override the active tenant
    #[arg(long, global = true, value_name = "SLUG", env = "FLUXBASE_TENANT")]
    tenant: Option<String>,

    /// Override the active project
    #[arg(long, global = true, value_name = "SLUG", env = "FLUXBASE_PROJECT")]
    project: Option<String>,

    /// Target environment (default: production)
    #[arg(long, global = true, value_name = "ENV", env = "FLUXBASE_ENV", default_value = "production")]
    env: String,

    /// Output raw JSON (machine-readable)
    #[arg(long, global = true)]
    json: bool,

    /// Disable coloured output
    #[arg(long, global = true)]
    no_color: bool,

    /// Suppress non-error output
    #[arg(long, global = true)]
    quiet: bool,

    /// Enable verbose/debug output
    #[arg(long, global = true)]
    verbose: bool,

    /// Show what would happen without making changes
    #[arg(long, global = true)]
    dry_run: bool,

    /// Auto-confirm prompts (non-interactive)
    #[arg(long, global = true)]
    yes: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum BugCommands {
    /// Binary-search commits to find the first regression (reads trace history, no replays needed).
    Bisect {
        /// Function name to bisect (e.g. create_user)
        #[arg(long)]
        function: String,
        /// Known-good commit SHA (prefix is fine)
        #[arg(long)]
        good: String,
        /// Known-bad commit SHA (prefix is fine)
        #[arg(long)]
        bad: String,
        /// Failure-rate threshold to classify a commit as bad (0.0–1.0, default: 0.05)
        #[arg(long, default_value = "0.05")]
        threshold: f64,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum Commands {
    /// Authenticate with Fluxbase
    Login,
    /// Show the current authenticated identity
    Whoami,

    // ── Tenants & Projects ────────────────────────────────────────────────────
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

    // ── Functions ─────────────────────────────────────────────────────────────
    /// Function operations (create scaffold, list)
    Function {
        #[command(subcommand)]
        command: functions::FunctionCommands,
    },
    /// Deploy to Fluxbase.
    ///
    /// In a function directory (has flux.json): deploys that single function.
    /// At the project root: discovers and deploys all function sub-directories.
    Deploy {
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        runtime: Option<String>,
    },
    /// Invoke a deployed function
    Invoke {
        name: String,
        #[arg(long, value_name = "JSON")]
        payload: Option<String>,
        #[arg(long)]
        gateway: bool,
    },
    /// Deployment version operations (list, rollback, promote, diff)
    Version {
        #[command(subcommand)]
        command: version_cmd::VersionCommands,
    },
    /// Deployment history
    Deployments {
        #[command(subcommand)]
        command: deployments::DeploymentCommands,
    },
    /// Run function locally with file-watch hot-reload
    Dev,

    // ── Scaffolding ───────────────────────────────────────────────────────────
    /// Create a new project from an official template
    New {
        name: String,
        #[arg(long, short, value_name = "TEMPLATE")]
        template: Option<String>,
    },
    /// Alias for `new` (backward-compatible)
    #[command(hide = true)]
    Create {
        name: String,
        #[arg(long, short, value_name = "TEMPLATE")]
        template: Option<String>,
    },
    /// Initialise .fluxbase/config.json for this project
    Init {
        #[arg(long, value_name = "PROJECT_ID")]
        project: Option<String>,
        #[arg(long, value_name = "FILE")]
        output: Option<String>,
        #[arg(long, value_name = "SECS")]
        interval: Option<u64>,
        #[arg(long, value_name = "URL")]
        api_url: Option<String>,
        #[arg(long, value_name = "URL")]
        gateway_url: Option<String>,
        #[arg(long, value_name = "URL")]
        runtime_url: Option<String>,
    },

    /// Dry-run a query: show the compiler output, applied policies, complexity score, and
    /// compiled SQL without executing against the database.
    ///
    /// Examples:
    ///   flux explain request.json
    ///   flux explain -          (read JSON from stdin)
    ///   flux explain request.json --json
    Explain {
        /// Path to a JSON query file (use "-" for stdin)
        file: Option<std::path::PathBuf>,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },

    /// Compare two executions of the same request: duration, status, and field-level state diffs.
    ///
    /// Typical use: compare original production run vs a replay to see what changed.
    ///
    /// Examples:
    ///   flux trace diff 9624a58d 550e8400
    ///   flux trace diff 9624a58d 550e8400 --json
    TraceDiff {
        /// Original request ID (production run)
        original_id: String,
        /// Second request ID to compare against (typically a replay)
        replay_id: String,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
        /// Only diff mutations for this table (dramatically faster on large traces)
        #[arg(long)]
        table: Option<String>,
    },

    /// Step-through debugger for a past production request.
    ///
    /// Walks the execution graph span-by-span, showing which database mutations
    /// happened at each step.  Reconstructs backend state at any point in execution.
    ///
    /// Examples:
    ///   flux trace debug 9624a58d
    ///   flux trace debug 9624a58d --at 2
    ///   flux trace debug 9624a58d --interactive
    ///   flux trace debug 9624a58d --json
    TraceDebug {
        /// Request ID to debug
        trace_id: String,
        /// Inspect state at exactly this step number (1-based)
        #[arg(long, value_name = "STEP")]
        at: Option<usize>,
        /// Step through spans interactively (Enter/s/p/q)
        #[arg(long)]
        interactive: bool,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },

    /// Binary-search commit history to find the first regression.
    ///
    /// Scans trace history for a function, groups by commit SHA, and binary-searches
    /// between a known-good and known-bad commit to identify the first bad deploy.
    ///
    /// Examples:
    ///   flux bug bisect --function create_user --good a93f42c --bad 9624a58d
    ///   flux bug bisect --function create_user --good a93f42c --bad 9624a58d --threshold 0.1
    Bug {
        #[command(subcommand)]
        command: BugCommands,
    },

    /// Root cause explanation for a request — shows error, commit, state changes, and suggested fixes.
    ///
    /// Combines trace spans, state mutations, and error logs for a single request_id.
    ///
    /// Examples:
    ///   flux why 550e8400
    ///   flux why 550e8400 --json
    Why {
        /// Request ID to explain
        request_id: String,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },

    /// Row-level state audit (history + blame).
    ///
    /// Examples:
    ///   flux state history users --id 42
    ///   flux state blame orders
    State {
        #[command(subcommand)]
        command: state::StateCommands,
    },

    /// Incident investigation and deterministic replay.
    ///
    /// Examples:
    ///   flux incident replay 2026-03-11T15:00:00Z..2026-03-11T15:05:00Z
    ///   flux incident replay --request-id 550e8400
    Incident {
        #[command(subcommand)]
        command: incident::IncidentCommands,
    },

    // ── Observability ─────────────────────────────────────────────────────────
    /// Tail or stream platform logs
    Logs {
        source: Option<String>,
        resource: Option<String>,
        #[arg(short, long)]
        follow: bool,
        #[arg(long, default_value = "100", value_name = "N")]
        limit: u64,
    },
    /// Show the full cross-service request trace for a request ID
    Trace {
        request_id: String,
        #[arg(long, default_value = "500", value_name = "MS")]
        slow: u64,
        #[arg(long)]
        flame: bool,
        /// Re-apply this request in replay mode (x-flux-replay: true)
        #[arg(long)]
        replay: bool,
    },
    /// Interactive production debugger.
    ///
    /// Without a request ID: lists recent errors and lets you select one.
    /// With a request ID: deep-dives that specific request.
    ///
    /// Examples:
    ///   flux debug               # interactive: pick from recent errors
    ///   flux debug 9624a58d57e7  # deep-dive a specific request
    Debug {
        /// Request ID to inspect directly (omit for interactive mode)
        request_id: Option<String>,
        #[arg(long)]
        replay: bool,
        #[arg(long, value_name = "FILE")]
        replay_payload: Option<String>,
        #[arg(long)]
        no_logs: bool,
        #[arg(long)]
        json: bool,
    },
    /// Alias for `debug` — shorter to type when responding to an alert.
    ///
    /// `flux fix` is identical to `flux debug`: interactive mode with no args,
    /// or deep-dive a specific request when a request ID is given.
    #[command(name = "fix")]
    Fix {
        request_id: Option<String>,
        #[arg(long)]
        replay: bool,
        #[arg(long, value_name = "FILE")]
        replay_payload: Option<String>,
        #[arg(long)]
        no_logs: bool,
        #[arg(long)]
        json: bool,
    },
    /// Production error summary by function — quick triage before `flux debug`.
    ///
    /// Shows per-function error counts, most recent error type, and p95 duration.
    ///
    /// Examples:
    ///   flux errors               # last 1h
    ///   flux errors --since 24h   # last 24h
    ///   flux errors --function create_user
    Errors {
        /// Filter to a specific function
        #[arg(long, value_name = "NAME")]
        function: Option<String>,
        /// Time window (e.g. 1h, 24h, 7d)
        #[arg(long, default_value = "1h", value_name = "DURATION")]
        since: String,
        /// Output raw JSON
        #[arg(long)]
        json: bool,
    },
    /// Live request stream — htop for your backend.
    ///
    /// Streams incoming requests in real time: method, route, function, duration, status.
    /// Errors print a `flux debug <id>` hint inline.
    ///
    /// Examples:
    ///   flux tail                   # all functions
    ///   flux tail create_user       # single function
    ///   flux tail --errors          # errors only
    ///   flux tail --slow 500        # requests > 500ms
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
        /// Automatically run `flux debug` when an error appears (pauses stream)
        #[arg(long)]
        auto_debug: bool,
    },
    /// Monitor service status, metrics, and alerts
    Monitor {
        #[command(subcommand)]
        command: monitor::MonitorCommands,
    },

    // ── Secrets & Config ──────────────────────────────────────────────────────
    /// Secrets operations (list, set, delete)
    Secrets {
        #[command(subcommand)]
        command: secrets::SecretsCommands,
    },
    /// View and edit CLI/project configuration
    Config {
        #[command(subcommand)]
        command: config_cmd::ConfigCommands,
    },
    /// API key management (create, list, revoke, rotate)
    ApiKey {
        #[command(subcommand)]
        command: api_key::ApiKeyCommands,
    },

    // ── Gateway ───────────────────────────────────────────────────────────────
    /// Gateway route and middleware management
    Gateway {
        #[command(subcommand)]
        command: gateway::GatewayCommands,
    },

    // ── Workflows, Agents, Schedules ──────────────────────────────────────────
    /// Workflow operations (create, deploy, run, logs, trace)
    Workflow {
        #[command(subcommand)]
        command: workflow::WorkflowCommands,
    },
    /// AI Agent operations (create, deploy, run, simulate)
    Agent {
        #[command(subcommand)]
        command: agent::AgentCommands,
    },
    /// Scheduled job management (create, pause, resume, history)
    Schedule {
        #[command(subcommand)]
        command: schedule::ScheduleCommands,
    },
    /// Message queue management (create, publish, dlq)
    Queue {
        #[command(subcommand)]
        command: queue::QueueCommands,
    },
    /// Platform event operations (publish, subscribe, list, history)
    Event {
        #[command(subcommand)]
        command: event::EventCommands,
    },

    // ── Tools ────────────────────────────────────────────────────────────────
    /// Third-party tool integration (list, connect, disconnect, run)
    Tool {
        #[command(subcommand)]
        command: tool::ToolCommands,
    },

    // ── Environments ─────────────────────────────────────────────────────────
    /// Environment management (create, delete, clone)
    Env {
        #[command(subcommand)]
        command: env_cmd::EnvCommands,
    },

    // ── Database ─────────────────────────────────────────────────────────────
    /// Database operations (create, list, diff, query, shell, migration)
    Db {
        #[command(subcommand)]
        command: db::DbCommands,
    },

    // ── SDK ───────────────────────────────────────────────────────────────────
    /// Pull the TypeScript SDK for the current project
    Pull {
        #[arg(long, short, value_name = "FILE")]
        output: Option<String>,
    },
    /// Watch schema for changes and auto-regenerate the SDK
    Watch {
        #[arg(long, short, value_name = "FILE")]
        output: Option<String>,
        #[arg(long, default_value = "5", value_name = "SECS")]
        interval: u64,
    },
    /// Show local vs remote schema version status
    Status {
        #[arg(long, short, value_name = "FILE")]
        sdk: Option<String>,
    },

    // ── Local Stack ───────────────────────────────────────────────────────────
    /// Manage the local Fluxbase development stack (Docker Compose)
    Stack {
        #[command(subcommand)]
        command: StackCommand,
    },

    // ── Utilities ────────────────────────────────────────────────────────────
    /// Diagnose environment/connectivity, or analyze a failed request with flux doctor <request-id>
    Doctor {
        /// Request ID to diagnose (omit for environment health check)
        #[arg(value_name = "REQUEST_ID")]
        request_id: Option<String>,
    },
    /// Open the Fluxbase dashboard (or a specific resource) in the browser
    Open {
        #[command(subcommand)]
        command: Option<open::OpenCommands>,
    },
    /// Check for CLI updates and upgrade if needed
    Upgrade {
        #[arg(long)]
        check: bool,
        #[arg(long, value_name = "VERSION")]
        version: Option<String>,
    },
}

#[derive(Subcommand)]
enum StackCommand {
    /// Build and start all services (detached by default)
    Up {
        #[arg(long)]
        build: bool,
        #[arg(long)]
        foreground: bool,
    },
    /// Stop and remove containers
    Down {
        #[arg(short, long)]
        volumes: bool,
    },
    /// List running services and their exposed ports
    Ps,
    /// Tail logs for one or all services
    Logs {
        service: Option<String>,
        #[arg(long, default_value = "100")]
        tail: u32,
    },
    /// Wipe all local data volumes and restart fresh
    Reset,
    /// Run seed data against the running database service
    Seed {
        #[arg(long, value_name = "FILE")]
        file: Option<String>,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Propagate global flags via env vars so child modules can read them.
    // SAFETY: single-threaded at this point (before tokio spawns tasks).
    if cli.no_color {
        unsafe { std::env::set_var("NO_COLOR", "1"); }
    }
    if let Some(t) = &cli.tenant {
        unsafe { std::env::set_var("FLUXBASE_TENANT", t); }
    }
    if let Some(p) = &cli.project {
        unsafe { std::env::set_var("FLUXBASE_PROJECT", p); }
    }

    match cli.command {
        Commands::Login   => auth::execute().await?,
        Commands::Whoami  => whoami::execute().await?,

        Commands::Tenant  { command } => tenant::execute(command).await?,
        Commands::Project { command } => projects::execute(command).await?,

        Commands::Function    { command } => functions::execute(command).await?,
        Commands::Deploy      { name, runtime } => deploy::execute(name, runtime).await?,
        Commands::Invoke      { name, payload, gateway } => invoke::execute(&name, None, payload, gateway).await?,
        Commands::Version     { command } => version_cmd::execute(command).await?,
        Commands::Deployments { command } => deployments::execute_deployments(command).await?,
        Commands::Dev                     => dev::execute().await?,

        Commands::New    { name, template } |
        Commands::Create { name, template } => create::execute(name, template).await?,

        Commands::Init { project, output, interval, api_url, gateway_url, runtime_url } => {
            init::execute(project, output, interval, api_url, gateway_url, runtime_url).await?
        }

        Commands::Logs { source, resource, follow, limit } => {
            const SOURCES: &[&str] = &["function", "db", "workflow", "event", "queue", "system"];
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
        Commands::Trace { request_id, slow, flame, replay } => {
            if replay {
                incident::execute(incident::IncidentCommands::Replay {
                    window:     None,
                    request_id: Some(request_id),
                    from:       None,
                    to:         None,
                    database:   "default".to_string(),
                    yes:        cli.yes,
                    json:       cli.json,
                }).await?
            } else {
                trace::execute(request_id, slow, flame).await?
            }
        }
        Commands::TraceDiff { original_id, replay_id, json, table } => trace_diff::execute(original_id, replay_id, json, table).await?,
        Commands::TraceDebug { trace_id, at, interactive, json } => trace_debug::execute(trace_id, at, interactive, json).await?,
        Commands::Bug { command } => match command {
            BugCommands::Bisect { function, good, bad, threshold, json } =>
                bisect::execute(function, good, bad, threshold, json).await?,
        },
        Commands::Why  { request_id, json } => why::execute(request_id, json).await?,
        Commands::Explain { file, json } => explain::execute(file, json).await?,
        Commands::State    { command } => state::execute(command).await?,
        Commands::Incident { command } => incident::execute(command).await?,

        Commands::Debug { request_id, replay, replay_payload, no_logs, json } |
        Commands::Fix   { request_id, replay, replay_payload, no_logs, json } => {
            debug::execute(request_id, replay, replay_payload, no_logs, json).await?
        }
        Commands::Tail { function, errors, slow, json, auto_debug } => {
            tail::execute(function, errors, slow, json, auto_debug).await?
        }
        Commands::Errors { function, since, json } => {
            errors::execute(function, since, json).await?
        }
        Commands::Monitor { command } => monitor::execute(command).await?,

        Commands::Secrets { command } => secrets::execute(command).await?,
        Commands::Config  { command } => config_cmd::execute(command).await?,
        Commands::ApiKey  { command } => api_key::execute(command).await?,

        Commands::Gateway  { command } => gateway::execute(command).await?,
        Commands::Workflow { command } => workflow::execute(command).await?,
        Commands::Agent    { command } => agent::execute(command).await?,
        Commands::Schedule { command } => schedule::execute(command).await?,
        Commands::Queue    { command } => queue::execute(command).await?,
        Commands::Event    { command } => event::execute(command).await?,
        Commands::Tool     { command } => tool::execute(command).await?,
        Commands::Env      { command } => env_cmd::execute(command).await?,

        Commands::Db { command } => db::execute(command).await?,

        Commands::Pull   { output }           => sdk::execute_pull(output).await?,
        Commands::Watch  { output, interval } => sdk::execute_watch(output, interval).await?,
        Commands::Status { sdk }              => sdk::execute_status(sdk).await?,

        Commands::Stack { command } => match command {
            StackCommand::Up    { build, foreground } => stack::execute_up(build, !foreground).await?,
            StackCommand::Down  { volumes }           => stack::execute_down(volumes).await?,
            StackCommand::Ps                          => stack::execute_ps().await?,
            StackCommand::Logs  { service, tail }     => stack::execute_logs(service, tail).await?,
            StackCommand::Reset                       => stack::execute_reset().await?,
            StackCommand::Seed  { file }              => stack::execute_seed(file).await?,
        },

        Commands::Doctor { request_id } => doctor::execute(request_id).await?,
        Commands::Open { command } => match command {
            Some(cmd) => open::execute(cmd).await?,
            None      => open::execute_default().await?,
        },
        Commands::Upgrade { check, version } => upgrade::execute(version, check).await?,
    }

    Ok(())
}
