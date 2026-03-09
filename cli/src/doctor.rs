//! `flux doctor` — environment health check.
//!
//! Inspects every layer of the developer environment and prints a concise
//! status report.  Designed to be the first command a developer runs when
//! something is behaving unexpectedly.
//!
//! ```text
//! $ flux doctor
//!
//! Fluxbase CLI doctor
//! ───────────────────────────────────
//! ✔ CLI version:      0.1.0
//! ✔ API reachable:    https://api.fluxbase.co  (62 ms)
//! ✔ Authenticated:    user@example.com
//! ✔ Tenant:           my-org  (tid_abc123)
//! ✔ Project:          proj_abc123  (from .fluxbase/config.json)
//! ✔ SDK file:         src/fluxbase.generated.ts
//!   └─ Schema:        v4  (hash: a3f8c1d2)  generated 2026-03-09T10:02:41Z
//! ⚠  SDK outdated:    local v4 → remote v5 — run: flux pull
//! ```

use std::path::PathBuf;
use std::time::Instant;

use colored::Colorize;
use reqwest::StatusCode;
use serde::Deserialize;

use crate::config::{Config, ProjectConfig};
use crate::sdk::parse_local_version;

// ─── Helper types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct MeResponse {
    email: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SchemaHealthResponse {
    schema_hash:    Option<String>,
    schema_version: Option<i64>,
}

// ─── Row printers ─────────────────────────────────────────────────────────────

fn ok(label: &str, value: &str) {
    println!("{}  {}  {}", "✔".green().bold(), label.bold(), value.cyan());
}

fn warn(label: &str, value: &str) {
    println!("{}  {}  {}", "⚠".yellow().bold(), label.bold(), value.yellow());
}

fn fail(label: &str, value: &str) {
    println!("{}  {}  {}", "✖".red().bold(), label.bold(), value.red());
}

fn info(text: &str) {
    println!("   {}", text.dimmed());
}

// ─── Handler ──────────────────────────────────────────────────────────────────

pub async fn execute() -> anyhow::Result<()> {
    println!();
    println!("{}", "Fluxbase CLI doctor".bold());
    println!("{}", "─".repeat(50).dimmed());

    // ── 1. CLI version ─────────────────────────────────────────────────────
    ok("CLI version:   ", env!("CARGO_PKG_VERSION"));

    // ── 2. Load global config ──────────────────────────────────────────────
    let config = Config::load().await;

    // ── 3. API reachability ────────────────────────────────────────────────
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .unwrap_or_default();

    let health_url = format!("{}/health", config.api_url);
    let t0 = Instant::now();
    match http.get(&health_url).send().await {
        Err(e) => {
            fail("API reachable:  ", &format!("{} — {}", config.api_url, e));
        }
        Ok(res) => {
            let ms = t0.elapsed().as_millis();
            if res.status().is_success() {
                ok(
                    "API reachable:  ",
                    &format!("{}  ({} ms)", config.api_url, ms),
                );
            } else {
                warn(
                    "API reachable:  ",
                    &format!(
                        "{}  HTTP {}  ({} ms)",
                        config.api_url,
                        res.status().as_u16(),
                        ms
                    ),
                );
            }
        }
    }

    // ── 4. Authentication ──────────────────────────────────────────────────
    let token = match &config.token {
        None => {
            fail("Authenticated:  ", "not logged in — run: flux login");
            // Cannot check anything further without a token.
            println!();
            return Ok(());
        }
        Some(t) => t.clone(),
    };

    // Build an authenticated client for subsequent checks.
    let mut auth_headers = reqwest::header::HeaderMap::new();
    if let Ok(v) = reqwest::header::HeaderValue::from_str(&format!("Bearer {}", token)) {
        auth_headers.insert(reqwest::header::AUTHORIZATION, v);
    }
    if let Some(tid) = &config.tenant_id {
        if let Ok(v) = reqwest::header::HeaderValue::from_str(tid) {
            auth_headers.insert("X-Fluxbase-Tenant", v);
        }
    }
    if let Some(pid) = &config.project_id {
        if let Ok(v) = reqwest::header::HeaderValue::from_str(pid) {
            auth_headers.insert("X-Fluxbase-Project", v);
        }
    }
    let auth_client = reqwest::Client::builder()
        .default_headers(auth_headers)
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .unwrap_or_default();

    // /auth/me
    let me_url = format!("{}/auth/me", config.api_url);
    match auth_client.get(&me_url).send().await {
        Err(e) => fail("Authenticated:  ", &format!("request failed — {}", e)),
        Ok(res) if res.status() == StatusCode::UNAUTHORIZED => {
            fail("Authenticated:  ", "token expired — run: flux login");
        }
        Ok(res) if !res.status().is_success() => {
            warn("Authenticated:  ", &format!("HTTP {}", res.status().as_u16()));
        }
        Ok(res) => {
            let body: MeResponse = res.json().await.unwrap_or(MeResponse { email: None });
            let email = body.email.as_deref().unwrap_or("(unknown)");
            ok("Authenticated:  ", email);
        }
    }

    // ── 5. Tenant ──────────────────────────────────────────────────────────
    match (&config.tenant_id, &config.tenant_slug) {
        (Some(tid), Some(slug)) => ok("Tenant:         ", &format!("{}  ({})", slug, tid)),
        (Some(tid), None)       => ok("Tenant:         ", tid),
        (None, _) => warn("Tenant:         ", "not set — run: flux tenant select"),
    }

    // ── 6. Project config ──────────────────────────────────────────────────
    let proj = ProjectConfig::load().await;
    match &config.project_id {
        None => warn("Project:        ", "not set — run: flux project select"),
        Some(pid) => {
            let source = if proj.as_ref().and_then(|p| p.project_id.as_deref()) == Some(pid.as_str()) {
                " (from .fluxbase/config.json)"
            } else {
                " (from ~/.fluxbase/config.json)"
            };
            ok("Project:        ", &format!("{}{}", pid, source.dimmed()));
        }
    }

    // Local project config file presence
    if proj.is_some() {
        if let Some(p) = ProjectConfig::find_path_pub() {
            info(&format!("└─ Config:  {}", p.display()));
        }
    } else {
        info(&format!(
            "└─ {}  (create with: flux init)",
            "No .fluxbase/config.json found in this directory".yellow()
        ));
    }

    // ── 7. URL overrides ───────────────────────────────────────────────────
    // Show resolved API + Gateway URLs so developers can confirm which
    // instance the CLI is pointed at.
    ok("API URL:        ", &config.api_url);
    ok("Gateway URL:    ", &config.gateway_url);
    let sdk_path_str = ProjectConfig::resolve_sdk_output(None, proj.as_ref());
    let sdk_path     = PathBuf::from(&sdk_path_str);

    if !sdk_path.exists() {
        warn(
            "SDK file:       ",
            &format!("{} (not found — run: flux pull)", sdk_path_str),
        );
    } else {
        ok("SDK file:       ", &sdk_path_str);

        // Parse embedded header
        let src = tokio::fs::read_to_string(&sdk_path).await.unwrap_or_default();
        if let Some((local_v, local_h)) = parse_local_version(&src) {
            // Try to extract the generation timestamp from the header too
            let gen_ts = src
                .lines()
                .take(15)
                .find_map(|l| l.trim().strip_prefix("* Generated:      "))
                .unwrap_or("?")
                .to_string();

            info(&format!(
                "└─ Schema:  v{}  (hash: {})  generated {}",
                local_v,
                &local_h[..local_h.len().min(8)],
                gen_ts.dimmed(),
            ));

            // ── 8. Remote schema comparison ────────────────────────────
            if config.project_id.is_some() {
                let schema_url = format!("{}/sdk/schema", config.api_url);
                match auth_client.get(&schema_url).send().await {
                    Err(e) => warn("Remote schema:  ", &format!("unreachable — {}", e)),
                    Ok(res) if !res.status().is_success() => {
                        warn(
                            "Remote schema:  ",
                            &format!("HTTP {}", res.status().as_u16()),
                        );
                    }
                    Ok(res) => {
                        let env: serde_json::Value = res.json().await.unwrap_or_default();
                        let inner = env.get("data").cloned().unwrap_or(env);
                        let remote: SchemaHealthResponse =
                            serde_json::from_value(inner).unwrap_or(SchemaHealthResponse {
                                schema_hash: None,
                                schema_version: None,
                            });

                        let remote_v    = remote.schema_version.unwrap_or(0);
                        let remote_hash = remote.schema_hash.as_deref().unwrap_or("");

                        let up_to_date =
                            local_v == remote_v && local_h == remote_hash;

                        if up_to_date {
                            ok("Remote schema:  ", &format!("v{}  — SDK is up to date", remote_v));
                        } else {
                            warn(
                                "Remote schema:  ",
                                &format!(
                                    "v{}  — SDK outdated (local v{})  → run: flux pull",
                                    remote_v, local_v
                                ),
                            );
                        }
                    }
                }
            }
        } else {
            info("└─ Schema:  header not found (file may be manually edited)");
        }
    }

    println!();
    Ok(())
}
