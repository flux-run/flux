use std::path::PathBuf;
use anyhow::{Context, Result, bail};
use clap::Args;
use crate::runtime_process::spawn_runtime;
use crate::events::FluxEvent;
use crate::run::{render, TuiApp};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event as CrossEvent, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Debug, Args)]
pub struct StartArgs {
    /// Path to a pre-built Flux artifact JSON (optional, defaults to .flux/artifact.json).
    #[arg(long, value_name = "FILE")]
    pub artifact: Option<String>,

    /// JSON input passed to the exported default handler.
    #[arg(long, value_name = "JSON", default_value = "{}")]
    pub input: String,

    /// Flux server URL for recording the execution (optional).
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,

    /// Service token for the Flux server (optional).
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,

    /// Use a release-mode flux-runtime binary if found.
    #[arg(long)]
    pub release: bool,

    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    #[arg(long, default_value_t = 3000)]
    pub port: u16,

    #[arg(long, default_value_t = 16)]
    pub isolate_pool_size: usize,
}

pub async fn execute(args: StartArgs) -> Result<()> {
    let cwd = std::env::current_dir().context("failed to get current directory")?;
    
    // 1. Resolve artifact path
    let artifact_path = if let Some(ref path) = args.artifact {
        PathBuf::from(path)
    } else {
        // Look for .flux/artifact.json in the current directory
        cwd.join(".flux").join("artifact.json")
    };

    if !artifact_path.exists() {
        bail!(
            "No build artifact found at {}.\nRun `flux build` first to create one.",
            artifact_path.display()
        );
    }

    // 2. Resolve project ID from flux.json
    let project_config = crate::project::load_project_config(&cwd).ok();
    let project_id = project_config.as_ref().and_then(|c| c.project_id.clone());
    let project_kind = project_config.as_ref().map(|c| c.kind.clone()).unwrap_or(shared::project::ProjectKind::Function);

    // 3. Prepare runtime arguments
    let binary = crate::bin_resolution::ensure_binary("flux-runtime", args.release).await?;
    let auth = crate::config::resolve_optional_auth(args.url.clone(), args.token.clone())?;

    let mut prog_args = vec![
        "--artifact".to_string(),
        artifact_path.to_string_lossy().to_string(),
        "--server-url".to_string(),
        auth.url.clone(),
        "--token".to_string(),
        auth.token.clone(),
        "--host".to_string(),
        args.host.clone(),
        "--port".to_string(),
        args.port.to_string(),
        "--isolate-pool-size".to_string(),
        args.isolate_pool_size.to_string(),
    ];

    if let Some(ref id) = project_id {
        prog_args.push("--project-id".to_string());
        prog_args.push(id.clone());
    }

    // If it's a function project, we usually want to serve it.
    if project_kind == shared::project::ProjectKind::Function {
        prog_args.push("--serve".to_string());
    }

    if !args.input.is_empty() && args.input != "{}" {
        prog_args.push("--script-input".to_string());
        prog_args.push(args.input.clone());
    }

    // 4. Setup TUI and run
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let project_name = cwd.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("flux-project")
        .to_string();
    let mut app = TuiApp::new(project_name, "artifact".to_string(), auth.url.clone());

    let mut child = spawn_runtime(binary, &prog_args, true).await?;
    let stdout = child.stdout.take().context("failed to take stdout")?;
    let mut reader = BufReader::new(stdout).lines();

    let mut done = false;
    while !done {
        tokio::select! {
            line_res = reader.next_line() => {
                match line_res {
                    Ok(Some(line)) => {
                        if line.starts_with("[flux-event] ") {
                            if let Some(event) = FluxEvent::from_json(&line[13..]) {
                                app.handle_event(event);
                            }
                        } else {
                            app.handle_event(FluxEvent::Log {
                                level: "info".to_string(),
                                message: line,
                            });
                        }
                        render(&mut terminal, &mut app)?;
                    }
                    _ => {
                        done = true;
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                if event::poll(std::time::Duration::from_millis(0))? {
                    if let CrossEvent::Key(key) = event::read()? {
                         if let KeyCode::Char('c') = key.code {
                            if key.modifiers.contains(event::KeyModifiers::CONTROL) {
                                done = true;
                            }
                        }
                    }
                }
                render(&mut terminal, &mut app)?;
            }
        }
    }

    // Cleanup TUI
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Some(exec) = app.executions.first() {
        let dashboard_url = std::env::var("FLUX_DASHBOARD_URL").unwrap_or_else(|_| "http://localhost:3000".to_string());
        let project_id_str = project_id.unwrap_or_else(|| "default".to_string());
        
        println!("\n  {} Execution Finished\n", if exec.status.as_deref() == Some("ok") { "✔" } else { "✘" });
        println!("  {} View in Dashboard:  {}/project/{}/executions/{}", "→", dashboard_url, project_id_str, exec.id);
        println!("  {} Replay locally:     flux replay {}", "→", exec.id);
        println!("  {} Debug root cause:   flux why {}\n", "→", exec.id);
    }

    Ok(())
}
