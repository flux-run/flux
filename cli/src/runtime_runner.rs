use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use crate::runtime_process::spawn_runtime;
use crate::events::FluxEvent;
use crate::tui::{TuiApp, render};

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event as CrossEvent, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

#[derive(Debug, PartialEq, Eq)]
pub enum RunResult {
    Finished,
    Restart,
}

pub struct RuntimeConfig {
    pub project_name: String,
    pub project_id: Option<String>,
    pub display_path: String,
    pub binary_path: PathBuf,
    pub args: Vec<String>,
    pub server_url: String,
    pub watch_dir: Option<PathBuf>,
    pub poll_ms: u64,
}

pub async fn run_with_tui(config: RuntimeConfig) -> Result<RunResult> {
    // 1. Setup TUI
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = TuiApp::new(config.project_name, config.display_path, config.server_url);

    // 2. Spawn and Run
    let mut child = spawn_runtime(config.binary_path, &config.args, true).await?;
    let runtime_stdout = child.stdout.take().context("failed to take stdout")?;
    let runtime_stderr = child.stderr.take().context("failed to take stderr")?;
    
    let mut stdout_reader = BufReader::new(runtime_stdout).lines();
    let mut stderr_reader = BufReader::new(runtime_stderr).lines();

    let mut stdout_done = false;
    let mut stderr_done = false;

    let mut fingerprint_before = if let Some(ref dir) = config.watch_dir {
        crate::project::watch_fingerprint(dir).ok()
    } else {
        None
    };

    let mut result = RunResult::Finished;

    while !stdout_done || !stderr_done {
        tokio::select! {
            line_res = stdout_reader.next_line(), if !stdout_done => {
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
                        stdout_done = true;
                    }
                }
            }
            line_res = stderr_reader.next_line(), if !stderr_done => {
                match line_res {
                    Ok(Some(line)) => {
                        app.handle_event(FluxEvent::Log {
                            level: "error".to_string(),
                            message: line,
                        });
                        render(&mut terminal, &mut app)?;
                    }
                    _ => {
                        stderr_done = true;
                    }
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {
                if event::poll(std::time::Duration::from_millis(0))? {
                    if let CrossEvent::Key(key) = event::read()? {
                         if let KeyCode::Char('c') = key.code {
                            if key.modifiers.contains(event::KeyModifiers::CONTROL) {
                                let _ = child.kill().await;
                                let _ = child.wait().await;
                                stdout_done = true;
                                stderr_done = true;
                            }
                        }
                    }
                }

                // Check for hot reload if enabled
                if let (Some(ref dir), Some(ref mut before)) = (&config.watch_dir, &mut fingerprint_before) {
                    if let Ok(now) = crate::project::watch_fingerprint(dir) {
                        if now != *before {
                            app.handle_event(FluxEvent::Log {
                                level: "info".to_string(),
                                message: "change detected, restarting...".to_string(),
                            });
                            let _ = child.kill().await;
                            let _ = child.wait().await;
                            stdout_done = true;
                            stderr_done = true;
                            result = RunResult::Restart;
                        }
                    }
                }

                render(&mut terminal, &mut app)?;
            }
        }
    }

    // If it was an immediate exit with errors (and no executions), wait for a keypress
    if app.executions.is_empty() && !app.system_logs.is_empty() {
        app.project_name = format!("{} (EXITED WITH ERRORS)", app.project_name);
        render(&mut terminal, &mut app)?;
        
        loop {
            if event::poll(std::time::Duration::from_millis(100))? {
                if let CrossEvent::Key(key) = event::read()? {
                    // Also catch Ctrl+C here
                    if let KeyCode::Char('c') = key.code {
                        if key.modifiers.contains(event::KeyModifiers::CONTROL) {
                            break;
                        }
                    }
                    break;
                }
            }
        }
    }

    // 3. Cleanup TUI
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Some(exec) = app.executions.first() {
        let dashboard_url = std::env::var("FLUX_DASHBOARD_URL").unwrap_or_else(|_| "https://fluxbase.co".to_string());
        let project_id_str = config.project_id.unwrap_or_else(|| "default".to_string());
        
        println!("\n  {} Execution Finished\n", if exec.status.as_deref() == Some("ok") { "✔" } else { "✘" });
        println!("  {} View in Dashboard:  {}/project/{}/executions/{}", "→", dashboard_url, project_id_str, exec.id);
        println!("  {} Replay locally:     flux replay {}", "→", exec.id);
        println!("  {} Debug root cause:   flux why {}\n", "→", exec.id);
    }

    Ok(result)
}
