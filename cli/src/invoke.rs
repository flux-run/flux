use crate::client::ApiClient;
use colored::Colorize;
use std::time::Instant;

// ── Invoke a deployed function ──────────────────────────────────
//
// Default mode: POST {runtime_url}/execute
//   Calls the runtime directly — fast, bypasses gateway auth+routing.
//
// --gateway mode: POST {gateway_url}/{function_name}
//   Routes through the full gateway stack (auth, rate-limit, analytics).
//   Uses X-Tenant header so the gateway can resolve identity.
//   Ensures production-parity testing.

pub async fn execute(
    name: &str,
    _tenant_slug: Option<String>,    // kept for back-compat but unused
    payload_str: Option<String>,
    via_gateway: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let config = &client.config;

    // Parse caller payload; default to empty object
    let payload: serde_json::Value = match payload_str.as_deref() {
        Some(s) => serde_json::from_str(s).map_err(|e| {
            anyhow::anyhow!("--payload is not valid JSON: {e}")
        })?,
        None => serde_json::json!({}),
    };

    if via_gateway {
        execute_via_gateway(name, config, payload).await
    } else {
        execute_via_runtime(name, config, payload).await
    }
}

// ── Runtime-direct path ────────────────────────────────────────────

async fn execute_via_runtime(
    name: &str,
    config: &crate::config::Config,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    let runtime_url = config.runtime_url.trim_end_matches('/').to_string();
    let exec_url = format!("{}/execute", runtime_url);

    let tenant_id = config
        .tenant_id
        .as_deref()
        .and_then(|s| s.parse::<uuid::Uuid>().ok())
        .ok_or_else(|| anyhow::anyhow!(
            "No tenant configured. Run `flux tenant use <slug>` first."
        ))?;

    let project_id: Option<uuid::Uuid> = config
        .project_id
        .as_deref()
        .and_then(|s| s.parse().ok());

    let body = serde_json::json!({
        "function_id": name,
        "tenant_id":   tenant_id,
        "project_id":  project_id,
        "payload":     payload,
    });

    print_invoke_header(name, &runtime_url, "runtime", &payload_str(&payload));

    let t0 = Instant::now();
    let http = reqwest::Client::new();
    let res = http
        .post(&exec_url)
        .header("X-Tenant-Id",   tenant_id.to_string())
        .header("X-Tenant-Slug", config.tenant_slug.as_deref().unwrap_or(""))
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Could not reach runtime at {}\n  {}", runtime_url, e))?;

    handle_response(name, res, t0.elapsed().as_millis()).await
}

// ── Gateway path ─────────────────────────────────────────────────

async fn execute_via_gateway(
    name: &str,
    config: &crate::config::Config,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    let gateway_url = config.gateway_url.trim_end_matches('/');

    let tenant_slug = config
        .tenant_slug
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!(
            "No tenant slug configured. Run `flux tenant use <slug>` first."
        ))?;

    // The gateway routes `POST /{function_name}` with X-Tenant: {slug}
    let exec_url = format!("{}/{}", gateway_url, name);

    print_invoke_header(name, gateway_url, "gateway", &payload_str(&payload));

    let t0 = Instant::now();
    let http = reqwest::Client::new();
    let res = http
        .post(&exec_url)
        .header("X-Tenant", tenant_slug)
        .json(&payload)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Could not reach gateway at {}\n  {}", gateway_url, e))?;

    handle_response(name, res, t0.elapsed().as_millis()).await
}

// ── Shared helpers ──────────────────────────────────────────────────

fn payload_str(v: &serde_json::Value) -> String {
    if v == &serde_json::json!({}) { String::new() }
    else { serde_json::to_string(v).unwrap_or_default() }
}

fn print_invoke_header(name: &str, url: &str, via: &str, payload_display: &str) {
    println!(
        "\n  {} Invoking {}  via {}  ({})",
        "▸".cyan(),
        name.bold(),
        via.yellow(),
        url.dimmed()
    );
    if !payload_display.is_empty() {
        println!("  payload: {}", payload_display.dimmed());
    }
    println!();
}

async fn handle_response(
    name: &str,
    res: reqwest::Response,
    elapsed_ms: u128,
) -> anyhow::Result<()> {
    let status = res.status();
    let cache_header = res
        .headers()
        .get("x-cache")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let response_body: serde_json::Value = res.json().await.unwrap_or(serde_json::json!({}));

    if status.is_success() {
        let dur = response_body
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .map(|ms| format!("  runtime {}ms  total {}ms", ms, elapsed_ms))
            .unwrap_or_else(|| format!("  {}ms", elapsed_ms));

        let cache_tag = if cache_header == "HIT" {
            format!("  {}", "(cached)".yellow())
        } else {
            String::new()
        };

        println!(
            "  {} {}{}{}",
            "✓".green().bold(),
            name.bold(),
            dur.dimmed(),
            cache_tag
        );
        println!();

        let result = response_body.get("result").unwrap_or(&response_body);
        let pretty = serde_json::to_string_pretty(result)?;
        for line in pretty.lines() {
            println!("  {}", line);
        }
        println!();
    } else {
        println!(
            "  {} {} (HTTP {}  {}ms)",
            "✗".red().bold(),
            name.bold(),
            status.as_u16().to_string().red(),
            elapsed_ms
        );
        println!();
        let pretty = serde_json::to_string_pretty(&response_body)?;
        for line in pretty.lines() {
            println!("  {}", line.red().to_string());
        }
        println!();

        anyhow::bail!("Function invocation failed with HTTP {}", status);
    }

    Ok(())
}

