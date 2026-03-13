//! `flux login` — authenticates with the local Flux server dashboard.
//!
//! On first run (no users exist) this transparently calls `POST /auth/setup`
//! to create the initial admin account, then saves the token to
//! `~/.flux/config.json` so subsequent CLI calls are authenticated.
//!
//! On subsequent runs it calls `POST /auth/login` with the supplied
//! email/password.
//!
//! The saved JWT is used by `ApiClient` as `Authorization: Bearer <token>`.

use anyhow::{Context, bail};
use colored::Colorize;
use serde::{Deserialize, Serialize};

use crate::client::ApiClient;
use crate::config::Config;

// ─── API shapes ───────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct SetupBody {
    username: String,
    email:    String,
    password: String,
    role:     &'static str,
}

#[derive(Serialize)]
struct LoginBody {
    email:    String,
    password: String,
}

#[derive(Deserialize)]
struct AuthResponse {
    token: String,
    user:  UserInfo,
}

#[derive(Deserialize)]
struct UserInfo {
    username: String,
    email:    String,
    role:     String,
}

#[derive(Deserialize)]
struct SetupStatus {
    user_count: u64,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn prompt(label: &str) -> anyhow::Result<String> {
    eprint!("{label}: ");
    let mut buf = String::new();
    std::io::stdin().read_line(&mut buf)?;
    Ok(buf.trim().to_owned())
}

fn prompt_password(label: &str) -> anyhow::Result<String> {
    rpassword::prompt_password(format!("{label}: "))
        .context("failed to read password")
}

async fn is_first_run(client: &ApiClient) -> bool {
    let res = client
        .client
        .get(format!("{}/auth/status", client.base_url))
        .send()
        .await;
    match res {
        Ok(r) if r.status().is_success() => {
            r.json::<SetupStatus>()
                .await
                .map(|s| s.user_count == 0)
                .unwrap_or(false)
        }
        _ => false,
    }
}

// ─── Public entry-point ───────────────────────────────────────────────────────

pub async fn execute() -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    // Check server is reachable before prompting for credentials.
    let health = client
        .client
        .get(format!("{}/health", client.base_url))
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await;

    if health.is_err() {
        eprintln!();
        eprintln!("  {} Cannot reach Flux server at {}", "✖".red(), client.base_url);
        eprintln!();
        eprintln!("  The server is not running. Start it first:");
        eprintln!("    {}", "flux dev".bold().cyan());
        eprintln!();
        bail!("server unreachable");
    }

    let first_run = is_first_run(&client).await;

    if first_run {
        // ── First-time setup ─────────────────────────────────────────────────
        println!("\n  {} No admin account found — creating the initial admin.\n", flux_ux::ROCKET);

        let username = {
            let u = prompt("  Admin username (default: admin)")?;
            if u.is_empty() { "admin".to_owned() } else { u }
        };
        let email    = prompt("  Admin email")?;
        if email.is_empty() { bail!("email cannot be empty"); }

        let password = prompt_password("  Password")?;
        if password.len() < 8 { bail!("password must be at least 8 characters"); }
        let confirm  = prompt_password("  Confirm password")?;
        if password != confirm { bail!("passwords do not match"); }

        let res = client
            .client
            .post(format!("{}/auth/setup", client.base_url))
            .json(&SetupBody { username, email, password, role: "admin" })
            .send()
            .await
            .context("request to /auth/setup failed")?;

        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            bail!("setup failed: {body}");
        }

        let auth: AuthResponse = res.json().await.context("invalid setup response")?;
        save_token(&auth.user.email, &auth.token).await?;

        println!("\n  {} Admin account created!", flux_ux::CHECK);
        println!("  Email:    {}", auth.user.email);
        println!("  Username: {}", auth.user.username);
        println!("  Role:     {}", auth.user.role);
        println!("\n  Token saved. Open the dashboard at http://localhost:4000/flux\n");
    } else {
        // ── Normal login ─────────────────────────────────────────────────────
        println!("\n  Login to Flux dashboard\n");

        let email    = prompt("  Email")?;
        if email.is_empty() { bail!("email cannot be empty"); }
        let password = prompt_password("  Password")?;

        let res = client
            .client
            .post(format!("{}/auth/login", client.base_url))
            .json(&LoginBody { email, password })
            .send()
            .await
            .context("request to /auth/login failed")?;

        if res.status() == reqwest::StatusCode::UNAUTHORIZED {
            bail!("incorrect email or password");
        }
        if !res.status().is_success() {
            let body = res.text().await.unwrap_or_default();
            bail!("login failed: {body}");
        }

        let auth: AuthResponse = res.json().await.context("invalid login response")?;
        save_token(&auth.user.email, &auth.token).await?;

        println!("\n  {} Logged in as {} ({})\n", flux_ux::CHECK, auth.user.username, auth.user.role);
    }

    Ok(())
}

/// Persist the JWT as `cli_key` in `~/.flux/config.json`.
async fn save_token(_email: &str, token: &str) -> anyhow::Result<()> {
    let mut cfg = Config::load().await;
    cfg.cli_key = Some(token.to_owned());
    cfg.save().await.context("failed to save token to ~/.flux/config.json")?;
    Ok(())
}

// ─── Tiny UX constants ────────────────────────────────────────────────────────

mod flux_ux {
    pub const ROCKET: &str = "🚀";
    pub const CHECK:  &str = "✓";
}



