use crate::client::ApiClient;
use colored::Colorize;
use std::time::Instant;

// ── Invoke a deployed function ──────────────────────────────────────────────
//
// Default mode: POST {runtime_url}/execute  {"function_id": …, "payload": …}
//   Direct to runtime — fastest path for local dev, no middleware.
//
// --gateway flag: POST {gateway_url}/{function_name}  {payload}
//   Routes through the full gateway stack (routing, rate-limiting, middleware).
//   Use this to test production-parity behaviour locally.

pub async fn execute(
    name: &str,
    _removed: Option<String>, // formerly tenant_slug — kept only for call-site compat
    payload_str: Option<String>,
    via_gateway: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    // Parse caller payload; default to empty object
    let payload: serde_json::Value = match payload_str.as_deref() {
        Some(s) => serde_json::from_str(s)
            .map_err(|e| anyhow::anyhow!("--payload is not valid JSON: {e}"))?,
        None => serde_json::json!({}),
    };

    if via_gateway {
        invoke_via_gateway(&client, name, payload).await
    } else {
        invoke_via_runtime(&client, name, payload).await
    }
}

// ── Runtime-direct path ────────────────────────────────────────────

async fn invoke_via_runtime(
    client: &ApiClient,
    name: &str,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    // In the monolithic server the runtime is in-process.
    // The /flux/dev/invoke/:name endpoint bypasses gateway routing so the
    // function works without needing `flux deploy` first.
    let api_url = client.base_url.trim_end_matches('/');
    // base_url is http://localhost:4000/flux/api — step up to the server root
    let base = api_url.trim_end_matches("/flux/api").trim_end_matches('/');
    let exec_url = format!("{}/flux/dev/invoke/{}", base, name);

    print_invoke_header(name, base, "dev", &payload_str(&payload));

    let t0 = Instant::now();
    let res = client
        .client
        .post(&exec_url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!(
            "Could not reach server at {}\n  {}\n  Is `flux dev` running?",
            base, e
        ))?;

    handle_response(name, res, t0.elapsed().as_millis()).await
}

// ── Gateway path ─────────────────────────────────────────────────

async fn invoke_via_gateway(
    client: &ApiClient,
    name: &str,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    let gateway_url = client.gateway_url.trim_end_matches('/');
    // Local single-tenant gateway: POST /{function_name} with the payload
    let exec_url = format!("{}/{}", gateway_url, name);

    print_invoke_header(name, gateway_url, "gateway", &payload_str(&payload));

    let t0 = Instant::now();
    let res = client
        .client
        .post(&exec_url)
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

