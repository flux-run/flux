use crate::client::ApiClient;
use colored::Colorize;
use serde_json::Value;

// ── Timestamp helpers ─────────────────────────────────────────────────────────

/// Format an RFC-3339 timestamp to HH:MM:SS.mmm for display.
fn format_timestamp(ts: &str) -> String {
    // "2026-03-10T10:01:12.031000Z" → "10:01:12.031"
    let time = ts.split('T').nth(1).unwrap_or(ts);
    let time = time.trim_end_matches('Z');
    let mut parts = time.splitn(3, ':');
    let h = parts.next().unwrap_or("00");
    let m = parts.next().unwrap_or("00");
    let rest = parts.next().unwrap_or("00");
    let s   = rest.split('.').next().unwrap_or("00");
    let ms  = rest.split('.').nth(1).map(|f| &f[..f.len().min(3)]).unwrap_or("000");
    format!("{h}:{m}:{s}.{ms:0<3}")
}

/// Parse an RFC-3339 timestamp to milliseconds-since-midnight.
/// Sufficient for single-request traces that don't span midnight.
fn ts_to_ms(ts: &str) -> Option<u64> {
    let time = ts.split('T').nth(1)?;
    let time = time.trim_end_matches('Z');
    let mut parts = time.splitn(3, ':');
    let h: u64 = parts.next()?.parse().ok()?;
    let m: u64 = parts.next()?.parse().ok()?;
    let rest   = parts.next()?;
    let mut rp = rest.splitn(2, '.');
    let s: u64  = rp.next()?.parse().ok()?;
    let ms: u64 = if let Some(frac) = rp.next() {
        let t = &frac[..frac.len().min(3)];
        format!("{:0<3}", t).parse().unwrap_or(0)
    } else { 0 };
    Some(h * 3_600_000 + m * 60_000 + s * 1_000 + ms)
}

// ── Span type helpers ─────────────────────────────────────────────────────────

fn span_icon(span_type: &str) -> &'static str {
    match span_type {
        "start" => "▶",
        "end"   => "■",
        "error" => "✗",
        _       => "·",
    }
}

fn colorize_span_icon(span_type: &str, icon: &str) -> colored::ColoredString {
    match span_type {
        "start" => icon.cyan().bold(),
        "end"   => icon.green().bold(),
        "error" => icon.red().bold(),
        _       => icon.dimmed(),
    }
}

fn colorize_source(source: &str) -> colored::ColoredString {
    match source {
        "gateway"  => source.blue(),
        "api"      => source.cyan(),
        "db"       => source.magenta(),
        "workflow" => source.yellow(),
        "queue"    => source.blue(),
        "system"   => source.dimmed(),
        _          => source.green(),
    }
}

fn colorize_level(level: &str) -> colored::ColoredString {
    match level.to_uppercase().as_str() {
        "ERROR" | "ERR"     => level.to_uppercase().red().bold(),
        "WARN"  | "WARNING" => level.to_uppercase().yellow().bold(),
        "DEBUG"             => level.to_uppercase().dimmed(),
        _                   => level.to_uppercase().normal(),
    }
}

// ── Main entry point ──────────────────────────────────────────────────────────

pub async fn execute(request_id: String) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let url    = format!("{}/traces/{}", client.base_url, request_id);

    let res: reqwest::Response = client.client.get(&url).send().await?;

    if res.status() == reqwest::StatusCode::NOT_FOUND {
        eprintln!("{} no trace found for request ID: {}", "✗".red(), request_id.bold());
        return Ok(());
    }
    if !res.status().is_success() {
        anyhow::bail!("API error: {}", res.status());
    }

    let body: Value = res.json().await?;
    let data   = &body["data"];
    let spans  = data["spans"].as_array().cloned().unwrap_or_default();
    let count  = data["span_count"].as_u64().unwrap_or(spans.len() as u64);

    if spans.is_empty() {
        println!("{} No spans found for request ID: {}", "ℹ".blue(), request_id.cyan().bold());
        println!("  Make sure the request was routed through the gateway and logs are enabled.");
        return Ok(());
    }

    // ── Header ────────────────────────────────────────────────────────────────
    let first_ms = spans.first().and_then(|s| s["timestamp"].as_str()).and_then(ts_to_ms);
    let last_ms  = spans.last().and_then(|s| s["timestamp"].as_str()).and_then(ts_to_ms);
    let total_ms = match (first_ms, last_ms) {
        (Some(f), Some(l)) if l >= f => Some(l - f),
        _ => None,
    };

    println!();
    println!("{} {}", "Trace".bold().white(), request_id.cyan().bold());
    match total_ms {
        Some(t) => println!("{} spans  •  {}ms end-to-end\n", count.to_string().dimmed(), t.to_string().bold()),
        None    => println!("{} spans\n", count.to_string().dimmed()),
    }

    // ── Span rows ─────────────────────────────────────────────────────────────
    // Pre-calculate column width for source/resource so columns stay aligned
    let label_width = spans.iter()
        .map(|s| {
            s["source"].as_str().unwrap_or("?").len()
            + 1
            + s["resource"].as_str().unwrap_or("?").len()
        })
        .max()
        .unwrap_or(18)
        .clamp(12, 32);

    let mut prev_ms: Option<u64> = None;

    for span in &spans {
        let ts        = span["timestamp"].as_str().unwrap_or("?");
        let source    = span["source"].as_str().unwrap_or("?");
        let resource  = span["resource"].as_str().unwrap_or("?");
        let level     = span["level"].as_str().unwrap_or("info");
        let message   = span["message"].as_str().unwrap_or("");
        let span_type = span["span_type"].as_str().unwrap_or("event");

        let cur_ms = ts_to_ms(ts);
        let delta  = match (prev_ms, cur_ms) {
            (Some(p), Some(c)) if c > p => format!("+{}ms", c - p).dimmed(),
            _                           => "      ".dimmed(),
        };
        prev_ms = cur_ms.or(prev_ms);

        // Pad the plain label to align columns, then colorise source separately
        let plain_label = format!("{}/{}", source, resource);
        let padded_plain = format!("{:<width$}", plain_label, width = label_width);
        // Replace the plain source prefix with the coloured version for display
        let coloured_label = padded_plain.replacen(
            source,
            &colorize_source(source).to_string(),
            1,
        );

        println!(
            "  {}  {}  {}  [{}]  {}  {}",
            format_timestamp(ts).dimmed(),
            delta,
            colorize_span_icon(span_type, span_icon(span_type)),
            coloured_label,
            colorize_level(level),
            message,
        );
    }

    // ── Footer ────────────────────────────────────────────────────────────────
    println!();
    match total_ms {
        Some(t) => println!("  {} spans  •  {}ms total", count, t.to_string().bold()),
        None    => println!("  {} spans", count),
    }
    println!();

    Ok(())
}
