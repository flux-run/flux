use anyhow::{Context, Result};
use clap::Args;
use std::io::{stdout, Write};
use crossterm::{
    cursor::{MoveToColumn, MoveToPreviousLine},
    event::{self, Event, KeyCode},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
};

use crate::config::CliConfig;
use crate::grpc::{normalize_grpc_url, validate_service_token};
use crate::project::scaffold_project;

#[derive(Debug, Args)]
pub struct InitArgs {
    #[arg(long)]
    pub auth: bool,

    #[command(subcommand)]
    pub command: Option<InitSubcommand>,

    #[arg(long)]
    pub force: bool,
}

#[derive(Debug, clap::Subcommand)]
pub enum InitSubcommand {
    /// Initialize a serverless function project (Cloud).
    Function,
    /// Initialize a standalone server project (Open Source).
    Server,
}

pub async fn execute(args: InitArgs) -> Result<()> {
    if args.auth {
        return init_auth().await;
    }

    let template = match &args.command {
        Some(InitSubcommand::Server) => "server",
        Some(InitSubcommand::Function) => "function",
        None => prompt_template()?,
    };
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    let config = CliConfig::load().unwrap_or_default();
    scaffold_project(&cwd, template, config.project_id, args.force)?;

    println!("\n  ✔  Project initialized successfully ({})\n", template);
    println!("  Created:");
    println!("    - ./flux.json");
    println!("    - ./src/index.ts\n");

    let config = CliConfig::load().unwrap_or_default();
    if config.token.is_some() {
        println!("  Authentication:");
        println!("    ✔  Logged in to {}", config.url.unwrap_or_default());
    } else {
        println!("  Next steps:");
        println!("    1. flux login  (to connect to Flux Cloud)");
    }

    println!("\n  Development:");
    println!("    - flux dev     (start the local development server)");

    Ok(())
}

fn prompt_template() -> Result<&'static str> {
    let options = ["function", "server"];
    let mut selected = 0;

    println!("? Select a project template:");
    println!("  function (Cloud - Serverless)");
    println!("  server   (Open Source - Standalone)");

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, crossterm::cursor::Hide)?;

    let result: Result<&'static str> = (|| loop {
        // Move back to the start of the options
        execute!(stdout, MoveToPreviousLine(2), MoveToColumn(0))?;
        
        for (i, option) in options.iter().enumerate() {
            // Clear the line and ensure we're at column 0
            execute!(stdout, Clear(ClearType::CurrentLine), MoveToColumn(0))?;
            
            if i == selected {
                execute!(
                    stdout,
                    SetForegroundColor(Color::Cyan),
                    Print("-> "),
                    Print(option),
                    ResetColor,
                    Print("\r\n")
                )?;
            } else {
                execute!(
                    stdout,
                    Print("   "),
                    Print(option),
                    Print("\r\n")
                )?;
            }
        }

        // Handle Input
        if let Event::Key(key_event) = event::read()? {
            match key_event.code {
                KeyCode::Up => {
                    if selected > 0 {
                        selected -= 1;
                    }
                }
                KeyCode::Down => {
                    if selected < options.len() - 1 {
                        selected += 1;
                    }
                }
                KeyCode::Enter => break Ok(options[selected]),
                KeyCode::Char('c') if key_event.modifiers.contains(event::KeyModifiers::CONTROL) => {
                    return Err(anyhow::anyhow!("Operation cancelled"));
                }
                _ => {}
            }
        }
    })();

    execute!(stdout, crossterm::cursor::Show)?;
    disable_raw_mode()?;
    println!(); 

    result
}

async fn init_auth() -> Result<()> {
    println!("Flux auth init\n");

    let mut url = String::new();
    print!("Server URL (default: localhost:50051): ");
    std::io::stdout().flush().ok();
    std::io::stdin()
        .read_line(&mut url)
        .context("failed to read server URL")?;
    let url = {
        let trimmed = url.trim();
        if trimmed.is_empty() {
            "localhost:50051".to_string()
        } else {
            trimmed.to_string()
        }
    };

    let token =
        rpassword::prompt_password("Service token: ").context("failed to read service token")?;

    let normalized_url = normalize_grpc_url(&url);
    let auth_result = validate_service_token(&normalized_url, &token).await?;

    let config = CliConfig {
        url: Some(normalized_url.clone()),
        token: Some(token),
        project_id: auth_result.project_id, 
    };
    config.save()?;

    println!("\n✓ saved config to ~/.flux/config.toml");
    println!("  server: {}", normalized_url);
    println!("  auth:   {}", auth_result.auth_mode);

    Ok(())
}
