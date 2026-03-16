use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use clap::Args;

use crate::config::resolve_auth;
use crate::grpc::validate_service_token;

#[derive(Debug, Args)]
pub struct ServeArgs {
    #[arg(value_name = "ENTRY", default_value = "index.js")]
    pub entry: String,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
    #[arg(long)]
    pub skip_verify: bool,
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, default_value_t = 3000)]
    pub port: u16,
    #[arg(long, default_value_t = 16)]
    pub isolate_pool_size: usize,
    #[arg(long)]
    pub check_only: bool,
}

pub async fn execute(args: ServeArgs) -> Result<()> {
    let auth = resolve_auth(args.url, args.token)?;
    let auth_mode = if args.skip_verify {
        "skipped".to_string()
    } else {
        validate_service_token(&auth.url, &auth.token).await?
    };

    let entry = PathBuf::from(&args.entry);
    if !entry.exists() {
        bail!("entry file not found: {}", entry.display());
    }

    let code = load_entry_code(&entry)?;
    let name = entry
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid entry file name: {}", entry.display()))?;
    let artifact = runtime::build_artifact(name, code);

    println!("runtime artifact prepared");
    println!("server:   {}", auth.url);
    println!("auth:     {}", auth_mode);
    println!("entry:    {}", entry.display());
    println!("hash:     {}", artifact.sha256);
    println!("bytes:    {}", artifact.size_bytes);
    if args.check_only {
        println!("status:   ready for runtime execution and event streaming");
        return Ok(());
    }

    let route_name = entry
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("invalid entry file stem: {}", entry.display()))?
        .to_string();

    let runtime_url = format!("http://{}:{}/{}", args.host, args.port, route_name);
    println!("runtime:  {}", runtime_url);
    println!("status:   serving");

    runtime::run_http_runtime(
        runtime::HttpRuntimeConfig {
            host: args.host,
            port: args.port,
            route_name,
            isolate_pool_size: args.isolate_pool_size,
        },
        artifact,
    )
    .await?;

    Ok(())
}

fn load_entry_code(entry: &Path) -> Result<String> {
    match extension(entry).as_deref() {
        Some("js") | Some("mjs") | Some("cjs") => fs::read_to_string(entry)
            .with_context(|| format!("failed to read {}", entry.display())),
        Some("ts") | Some("tsx") => transpile_typescript(entry),
        _ => bail!("unsupported entry type: {}", entry.display()),
    }
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
}

fn transpile_typescript(entry: &Path) -> Result<String> {
    let temp_root = std::env::temp_dir().join(format!(
        "flux-ts-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&temp_root)
        .with_context(|| format!("failed to create {}", temp_root.display()))?;

    let attempts: [(&str, &[&str]); 3] = [
        (
            "npx",
            &[
                "--yes",
                "tsc",
                "--pretty",
                "false",
                "--target",
                "es2022",
                "--module",
                "es2022",
                "--outDir",
            ],
        ),
        (
            "bunx",
            &[
                "tsc",
                "--pretty",
                "false",
                "--target",
                "es2022",
                "--module",
                "es2022",
                "--outDir",
            ],
        ),
        (
            "tsc",
            &[
                "--pretty",
                "false",
                "--target",
                "es2022",
                "--module",
                "es2022",
                "--outDir",
            ],
        ),
    ];

    let mut last_error = String::new();
    for (bin, prefix_args) in attempts {
        let mut command = Command::new(bin);
        command.args(prefix_args);
        command.arg(&temp_root);
        command.arg(entry);

        match command.output() {
            Ok(output) if output.status.success() => {
                let js_path = temp_root.join(
                    entry.file_stem()
                        .and_then(|value| value.to_str())
                        .ok_or_else(|| anyhow::anyhow!("invalid TypeScript file name: {}", entry.display()))?
                        .to_string() + ".js",
                );

                let code = fs::read_to_string(&js_path)
                    .with_context(|| format!("failed to read transpiled output {}", js_path.display()))?;
                let _ = std::fs::remove_dir_all(&temp_root);
                return Ok(code);
            }
            Ok(output) => {
                last_error = String::from_utf8_lossy(&output.stderr).trim().to_string();
            }
            Err(err) => {
                last_error = err.to_string();
            }
        }
    }

    let _ = std::fs::remove_dir_all(&temp_root);
    bail!(
        "failed to transpile TypeScript {}; tried npx, bunx, and tsc: {}",
        entry.display(),
        last_error
    )
}