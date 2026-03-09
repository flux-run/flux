use crate::client::ApiClient;
use colored::Colorize;
use std::time::Instant;

// ── Invoke a deployed function through the runtime ────────────────────────
//
// Calls POST {runtime_url}/execute with:
//   function_id  — function name (runtime also accepts UUID)
//   tenant_id    — Uuid from config
//   project_id   — Uuid from config (optional)
//   payload      — arbitrary JSON from --payload flag

pub async fn execute(
    name: &str,
    _tenant_slug: Option<String>,    // kept for back-compat but unused
    payload_str: Option<String>,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let config = &client.config;

    let runtime_url = config.runtime_url.trim_end_matches('/').to_string();
    let exec_url = format!("{}/execute", runtime_url);

    // Parse tenant_id as UUID; bail early with a clear message if not configured
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

    // Parse caller payload; default to empty object
    let payload: serde_json::Value = match payload_str.as_deref() {
        Some(s) => serde_json::from_str(s).map_err(|e| {
            anyhow::anyhow!("--payload is not valid JSON: {e}")
        })?,
        None => serde_json::json!({}),
    };

    let body = serde_json::json!({
        "function_id": name,
        "tenant_id":   tenant_id,
        "project_id":  project_id,
        "payload":     payload,
    });

    println!(
        "\n  {} Invoking {}  ({})",
        "▸".cyan(),
        name.bold(),
        runtime_url.dimmed()
    );
    if !matches!(payload_str.as_deref(), None | Some("{}")) {
        println!("  payload: {}", serde_json::to_string(&payload)?.dimmed());
    }
    println!();

    let t0 = Instant::now();

    let res = client
        .client
        .post(&exec_url)
        .header("X-Tenant-Id",   tenant_id.to_string())
        .header(
            "X-Tenant-Slug",
            config.tenant_slug.as_deref().unwrap_or(""),
        )
        .json(&body)
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Could not reach runtime at {}\n  {}", runtime_url, e))?;

    let elapsed_ms = t0.elapsed().as_millis();
    let status = res.status();
    let response_body: serde_json::Value = res.json().await.unwrap_or(serde_json::json!({}));

    if status.is_success() {
        let dur = response_body
            .get("duration_ms")
            .and_then(|v| v.as_u64())
            .map(|ms| format!("  runtime {}ms  total {}ms", ms, elapsed_ms))
            .unwrap_or_else(|| format!("  {}ms", elapsed_ms));

        println!(
            "  {} {}  {}",
            "✓".green().bold(),
            name.bold(),
            dur.dimmed()
        );
        println!();

        // Pretty-print result
        let result = response_body.get("result").unwrap_or(&response_body);
        let pretty = serde_json::to_string_pretty(result)?;
        // Indent each line
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

