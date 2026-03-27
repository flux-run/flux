use anyhow::{Context, Result};
use clap::Args;
use crossterm::{
    cursor::{MoveToColumn, MoveToPreviousLine},
    event::{self, Event, KeyCode},
    execute,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType},
};
use std::io::stdout;

use crate::config::CliConfig;
use crate::grpc::{normalize_grpc_url, validate_service_token};

#[derive(Debug, Args)]
pub struct LoginArgs {
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
    #[arg(long)]
    pub skip_verify: bool,
}

pub async fn execute(args: LoginArgs) -> Result<()> {
    let (url, token) = match (args.url, args.token) {
        (Some(u), Some(t)) => (normalize_grpc_url(&u), t),
        (Some(u), None) => {
            let n_u = normalize_grpc_url(&u);
            let t = rpassword::prompt_password("Service token: ")
                .context("failed to read service token")?;
            (n_u, t)
        }
        _ => prompt_login()?,
    };

    if !args.skip_verify {
        let result = validate_service_token(&url, &token).await?;
        println!("✔ Logged in as developer@fluxbase.co");
        println!("✔ Server: {}", url);

        let config = CliConfig {
            url: Some(url.clone()),
            token: Some(token),
            project_id: result.project_id,
        };
        config.save()?;
    } else {
        let config = CliConfig {
            url: Some(url.clone()),
            token: Some(token),
            project_id: None,
        };
        config.save()?;
    }

    println!("saved CLI auth config");
    println!("server:  {}", url);

    Ok(())
}
fn prompt_login() -> Result<(String, String)> {
    let options = ["Cloud (api.fluxbase.co)", "Custom"];
    let mut selected = 0;

    println!("? Select an environment:");
    println!("  Cloud  (api.fluxbase.co)");
    println!("  Custom");

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, crossterm::cursor::Hide)?;

    let env_result: Result<usize> = (|| loop {
        // Move back to the start of options
        execute!(stdout, MoveToPreviousLine(2), MoveToColumn(0))?;

        for (i, option) in options.iter().enumerate() {
            // Clear line and reset column
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
                execute!(stdout, Print("   "), Print(option), Print("\r\n"))?;
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
                KeyCode::Enter => break Ok(selected),
                KeyCode::Char('c')
                    if key_event.modifiers.contains(event::KeyModifiers::CONTROL) =>
                {
                    return Err(anyhow::anyhow!("Operation cancelled"));
                }
                _ => {}
            }
        }
    })();

    execute!(stdout, crossterm::cursor::Show)?;
    disable_raw_mode()?;
    println!(); // Move to next line

    let env_choice = env_result?;

    let url = if env_choice == 0 {
        "https://api.fluxbase.co".to_string()
    } else {
        println!("? Server URL:");
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .context("failed to read server URL")?;
        input.trim().to_string()
    };

    let token =
        rpassword::prompt_password("Service token: ").context("failed to read service token")?;

    Ok((normalize_grpc_url(&url), token))
}
