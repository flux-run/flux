//! `flux gateway` — manage HTTP routing between the internet and your functions.

use clap::Subcommand;
use colored::Colorize;
use serde_json::Value;

use crate::client::ApiClient;

#[derive(Subcommand)]
pub enum GatewayCommands {
    /// Manage gateway routes
    Route {
        #[command(subcommand)]
        command: RouteCommands,
    },
    /// Manage gateway middleware
    Middleware {
        #[command(subcommand)]
        command: MiddlewareCommands,
    },
    /// Manage rate-limiting rules
    RateLimit {
        #[command(subcommand)]
        command: RateLimitCommands,
    },
    /// Manage CORS configuration
    Cors {
        #[command(subcommand)]
        command: CorsCommands,
    },
}

#[derive(Subcommand)]
pub enum RouteCommands {
    /// Create a new gateway route
    Create {
        /// URL path (e.g. /signup)
        #[arg(long)]
        path: String,
        /// HTTP method (GET | POST | PUT | DELETE | PATCH)
        #[arg(long, default_value = "POST")]
        method: String,
        /// Target function name
        #[arg(long)]
        function: String,
        /// Auth type (none | bearer | api-key)
        #[arg(long, default_value = "none")]
        auth: String,
        /// Fire-and-forget: queue the call and return 202 immediately
        #[arg(long)]
        r#async: bool,
    },
    /// List all gateway routes for the current project
    List,
    /// Get details of a specific route
    Get {
        /// Route ID (UUID prefix accepted)
        id: String,
    },
    /// Delete a gateway route
    Delete {
        /// Route ID (UUID prefix accepted)
        id: String,
        /// Skip confirmation prompt
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Subcommand)]
pub enum MiddlewareCommands {
    /// Attach middleware to a route
    Add {
        /// Route ID
        #[arg(long)]
        route: String,
        /// Middleware type (rate-limit | auth | cors | log)
        #[arg(long)]
        r#type: String,
        /// Middleware config as JSON
        #[arg(long)]
        config: Option<String>,
    },
    /// Remove middleware from a route
    Remove {
        /// Route ID
        #[arg(long)]
        route: String,
        /// Middleware type
        #[arg(long)]
        r#type: String,
    },
}

#[derive(Subcommand)]
pub enum RateLimitCommands {
    /// Set a rate limit on a route
    Set {
        #[arg(long)]
        route: String,
        /// Requests per second
        #[arg(long)]
        rps: u32,
        /// Burst size
        #[arg(long)]
        burst: Option<u32>,
    },
    /// Remove a rate limit from a route
    Remove {
        #[arg(long)]
        route: String,
    },
}

#[derive(Subcommand)]
pub enum CorsCommands {
    /// Set CORS allowed origins for a route
    Set {
        #[arg(long)]
        route: String,
        /// Comma-separated allowed origins
        #[arg(long)]
        origins: String,
    },
    /// List CORS settings for a route
    List {
        #[arg(long)]
        route: String,
    },
}

pub async fn execute(command: GatewayCommands) -> anyhow::Result<()> {
    match command {
        GatewayCommands::Route { command } => route_cmd(command).await,
        GatewayCommands::Middleware { command } => middleware_cmd(command).await,
        GatewayCommands::RateLimit { command } => rate_limit_cmd(command).await,
        GatewayCommands::Cors { command } => cors_cmd(command).await,
    }
}

// ── Route ──────────────────────────────────────────────────────────────────

async fn route_cmd(command: RouteCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    match command {
        RouteCommands::Create {
            path,
            method,
            function,
            auth,
            r#async: is_async,
        } => {
            let payload = serde_json::json!({
                "path": path,
                "method": method.to_uppercase(),
                "function_name": function,
                "auth_type": auth,
                "is_async": is_async,
            });

            let res = client
                .client
                .post(format!("{}/gateway/routes", client.base_url))
                .json(&payload)
                .send()
                .await?;

            let status = res.status();
            let json: Value = res.json().await.unwrap_or_default();

            if status.is_success() {
                let data = json.get("data").unwrap_or(&json);
                let id = data["id"].as_str().unwrap_or("?");
                println!(
                    "{} Route created: {} {} → {} ({})",
                    "✔".green().bold(),
                    method.to_uppercase().bold(),
                    path.cyan(),
                    function.bold(),
                    id.dimmed()
                );
            } else {
                let msg = json["error"]
                    .as_str()
                    .or_else(|| json["message"].as_str())
                    .unwrap_or("unknown error");
                anyhow::bail!("Failed to create route: {} — {}", status, msg);
            }
        }

        RouteCommands::List => {
            let res = client
                .client
                .get(format!("{}/gateway/routes", client.base_url))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let routes = json
                .get("data")
                .and_then(|d| d.get("routes").or(Some(d)))
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            if routes.is_empty() {
                println!("No routes configured.");
                println!("  {}", "flux gateway route create --help".dimmed());
            } else {
                println!(
                    "{:<38} {:<8} {:<25} {:<20} {:<8} ASYNC",
                    "ID".bold(),
                    "METHOD".bold(),
                    "PATH".bold(),
                    "FUNCTION".bold(),
                    "AUTH".bold()
                );
                println!("{}", "─".repeat(110).dimmed());
                for r in routes {
                    let method = r["method"].as_str().unwrap_or("");
                    let path = r["path"].as_str().unwrap_or("");
                    let func = r["function_name"].as_str().unwrap_or("");
                    let auth = r["auth_type"].as_str().unwrap_or("none");
                    let is_async = r["is_async"].as_bool().unwrap_or(false);
                    println!(
                        "{:<38} {:<8} {:<25} {:<20} {:<8} {}",
                        r["id"].as_str().unwrap_or(""),
                        method,
                        path,
                        func,
                        auth,
                        if is_async { "yes" } else { "no" }
                    );
                }
            }
        }

        RouteCommands::Get { id } => {
            let res = client
                .client
                .get(format!("{}/gateway/routes/{}", client.base_url, id))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            let data = json.get("data").unwrap_or(&json);
            println!("{}", serde_json::to_string_pretty(data)?);
        }

        RouteCommands::Delete { id, confirm } => {
            if !confirm {
                print!("Delete route {}? [y/N]: ", id.red());
                use std::io::{BufRead, Write};
                std::io::stdout().flush()?;
                let mut line = String::new();
                std::io::stdin().lock().read_line(&mut line)?;
                if line.trim().to_lowercase() != "y" {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            let res = client
                .client
                .delete(format!("{}/gateway/routes/{}", client.base_url, id))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Deleted route {}", "✔".green().bold(), id);
        }
    }
    Ok(())
}

// ── Middleware ──────────────────────────────────────────────────────────────

async fn middleware_cmd(command: MiddlewareCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    match command {
        MiddlewareCommands::Add { route, r#type, config } => {
            let cfg: Value = config
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .map_err(|e| anyhow::anyhow!("Invalid --config JSON: {}", e))?
                .unwrap_or(serde_json::json!({}));

            let payload = serde_json::json!({
                "route_id": route,
                "type": r#type,
                "config": cfg,
            });
            let res = client
                .client
                .post(format!("{}/gateway/middleware", client.base_url))
                .json(&payload)
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Middleware {} attached to route {}", "✔".green().bold(), r#type.cyan(), route.bold());
        }
        MiddlewareCommands::Remove { route, r#type } => {
            let res = client
                .client
                .delete(format!("{}/gateway/middleware/{}/{}", client.base_url, route, r#type))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Middleware {} removed from route {}", "✔".green().bold(), r#type.cyan(), route.bold());
        }
    }
    Ok(())
}

// ── Rate limit ──────────────────────────────────────────────────────────────

async fn rate_limit_cmd(command: RateLimitCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    match command {
        RateLimitCommands::Set { route, rps, burst } => {
            let payload = serde_json::json!({
                "route_id": route,
                "rps": rps,
                "burst": burst.unwrap_or(rps * 2),
            });
            let res = client
                .client
                .put(format!("{}/gateway/routes/{}/rate-limit", client.base_url, route))
                .json(&payload)
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Rate limit set: {} rps on route {}", "✔".green().bold(), rps.to_string().cyan(), route.bold());
        }
        RateLimitCommands::Remove { route } => {
            let res = client
                .client
                .delete(format!("{}/gateway/routes/{}/rate-limit", client.base_url, route))
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} Rate limit removed from route {}", "✔".green().bold(), route.bold());
        }
    }
    Ok(())
}

// ── CORS ─────────────────────────────────────────────────────────────────────

async fn cors_cmd(command: CorsCommands) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    match command {
        CorsCommands::Set { route, origins } => {
            let origin_list: Vec<&str> = origins.split(',').map(str::trim).collect();
            let payload = serde_json::json!({
                "route_id": route,
                "allowed_origins": origin_list,
            });
            let res = client
                .client
                .put(format!("{}/gateway/routes/{}/cors", client.base_url, route))
                .json(&payload)
                .send()
                .await?;
            res.error_for_status()?;
            println!("{} CORS set for route {}: {}", "✔".green().bold(), route.bold(), origins.cyan());
        }
        CorsCommands::List { route } => {
            let res = client
                .client
                .get(format!("{}/gateway/routes/{}/cors", client.base_url, route))
                .send()
                .await?;
            let json: Value = res.error_for_status()?.json().await?;
            println!("{}", serde_json::to_string_pretty(&json)?);
        }
    }
    Ok(())
}
