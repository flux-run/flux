use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use deno_ast::{EmitOptions, MediaType, ParseParams, TranspileModuleOptions, TranspileOptions};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "flux-runtime")]
#[command(about = "Flux runtime — runs user JS handlers and streams execution records to flux-server")]
struct Args {
    /// Entry file to serve (JS, MJS, CJS, TS, or TSX).
    #[arg(long, value_name = "FILE", default_value = "index.js")]
    entry: String,

    /// URL of the flux-server gRPC endpoint.
    #[arg(long, value_name = "URL", default_value = "http://127.0.0.1:50051")]
    server_url: String,

    /// Service token for authenticating with flux-server.
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN", default_value = "")]
    token: String,

    /// HTTP listen host.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// HTTP listen port.
    #[arg(long, default_value_t = 3000)]
    port: u16,

    /// Number of V8 isolates to keep warm.
    #[arg(long, default_value_t = 16)]
    isolate_pool_size: usize,

    /// Validate the entry file and print artifact info, then exit without serving.
    #[arg(long)]
    check_only: bool,

    /// Execute the entry file as a plain script (no HTTP server).
    /// Like `node index.js` — runs top-level code, drains the event loop, exits.
    #[arg(long)]
    script_mode: bool,

    /// JSON input passed to the exported default handler in script mode.
    /// Ignored when the entry file has no `export default` function.
    #[arg(long, value_name = "JSON", default_value = "{}")]
    script_input: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let args = Args::parse();

    let entry = PathBuf::from(&args.entry);
    if !entry.exists() {
        bail!("entry file not found: {}", entry.display());
    }

    // Validate extension whitelist before doing anything else.
    match extension(&entry).as_deref() {
        Some("js") | Some("mjs") | Some("cjs") | Some("ts") | Some("tsx") => {}
        _ => bail!("unsupported entry file extension: {}", entry.display()),
    }

    // Canonicalize and verify the entry file is within the current working directory
    // to prevent path-traversal attacks (e.g. --entry ../../etc/passwd).
    let canonical_entry = entry
        .canonicalize()
        .with_context(|| format!("failed to resolve entry path: {}", entry.display()))?;
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    let canonical_cwd = cwd
        .canonicalize()
        .context("failed to resolve current directory")?;
    if !canonical_entry.starts_with(&canonical_cwd) {
        bail!(
            "entry file must be within the working directory: {} is outside {}",
            canonical_entry.display(),
            canonical_cwd.display()
        );
    }

    let code = load_entry_code(&entry)?;
    let name = entry
        .file_name()
        .and_then(|v| v.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid entry file name: {}", entry.display()))?;
    let artifact = runtime::build_artifact(name, code);

    let route_name = entry
        .file_stem()
        .and_then(|v| v.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid entry file stem: {}", entry.display()))?
        .to_string();

    if args.script_mode {
        tracing::debug!(entry = %entry.display(), "script mode");
        let input: serde_json::Value = serde_json::from_str(&args.script_input)
            .with_context(|| format!("invalid --script-input JSON: {}", args.script_input))?;
        let mut isolate = runtime::JsIsolate::new_for_run(&artifact.code)
            .context("failed to create JS isolate")?;
        let (output, _logs) = isolate.run_script(input).await
            .context("script execution failed")?;
        if let Some(value) = output {
            if !value.is_null() {
                println!("{}", serde_json::to_string_pretty(&value).unwrap_or_default());
            }
        }
        return Ok(());
    }

    println!("server:   {}", args.server_url);
    println!("entry:    {}", entry.display());
    println!("hash:     {}", artifact.sha256);
    println!("bytes:    {}", artifact.size_bytes);
    println!("runtime:  http://{}:{}/{}", args.host, args.port, route_name);

    if args.check_only {
        println!("status:   ready for runtime execution and event streaming");
        return Ok(());
    }

    println!("status:   serving");

    runtime::run_http_runtime(
        runtime::HttpRuntimeConfig {
            host: args.host,
            port: args.port,
            route_name,
            isolate_pool_size: args.isolate_pool_size,
            server_url: args.server_url,
            service_token: args.token,
        },
        artifact,
    )
    .await?;

    Ok(())
}

fn load_entry_code(entry: &Path) -> Result<String> {
    match extension(entry).as_deref() {
        Some("js") | Some("mjs") | Some("cjs") => std::fs::read_to_string(entry)
            .with_context(|| format!("failed to read {}", entry.display())),
        Some("ts") | Some("tsx") => transpile_typescript(entry),
        _ => bail!("unsupported entry type: {}", entry.display()),
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase())
}

fn transpile_typescript(entry: &Path) -> Result<String> {
    let source = std::fs::read_to_string(entry)
        .with_context(|| format!("failed to read {}", entry.display()))?;

    let media_type = match extension(entry).as_deref() {
        Some("tsx") => MediaType::Tsx,
        _           => MediaType::TypeScript,
    };

    // Build a file:// specifier so SWC has a meaningful path in diagnostics.
    let specifier = url::Url::from_file_path(entry)
        .unwrap_or_else(|_| url::Url::parse("file:///input.ts").unwrap());

    let parsed = deno_ast::parse_module(ParseParams {
        specifier,
        text: source.into(),
        media_type,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    })
    .with_context(|| format!("failed to parse {}", entry.display()))?;

    let result = parsed
        .transpile(
            &TranspileOptions::default(),
            &TranspileModuleOptions::default(),
            &EmitOptions::default(),
        )
        .with_context(|| format!("failed to transpile {}", entry.display()))?;

    Ok(result.into_source().text)
}
