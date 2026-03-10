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

// ── Slow-span helpers ─────────────────────────────────────────────────────────

/// Colorise a delta label based on slow-span severity.
///   delta >= 2× threshold  →  red bold (critical)
///   delta >= threshold      →  yellow bold (slow)
///   otherwise               →  dimmed (normal)
fn colorize_delta(delta_ms: i64, slow_thresh: u64, is_slow: bool) -> colored::ColoredString {
    if delta_ms < 0 {
        return "      ".dimmed();
    }
    let label = format!("+{}ms", delta_ms);
    if !is_slow {
        return label.dimmed();
    }
    if delta_ms as u64 >= slow_thresh * 2 {
        label.red().bold()
    } else {
        label.yellow().bold()
    }
}

// ── Flame graph ──────────────────────────────────────────────────────────────

/// Render a Gantt-style waterfall timeline where every span is a row and its
/// position along the bar corresponds to `elapsed_ms / total_ms`.
fn render_flame(spans: &[Value], total_ms: i64, slow_thresh: u64) {
    const BAR_WIDTH: i64 = 52;
    const EMPTY: char = '░';
    const FILL: char  = '█';

    // Pre-calculate label column width (same logic as span table).
    let label_width = spans.iter()
        .map(|s| {
            s["source"].as_str().unwrap_or("?").len()
            + 1
            + s["resource"].as_str().unwrap_or("?").len()
        })
        .max()
        .unwrap_or(18)
        .clamp(12, 28);

    println!(
        "  {:<lw$}  {}  {}",
        "span".bold().to_string(),
        format!("|{:─<width$}|", "", width = BAR_WIDTH as usize).dimmed(),
        "Δ / message".dimmed(),
        lw = label_width,
    );
    println!(
        "  {:<lw$}  {}  {}",
        "",
        format!(" 0{:>width$}", format!("{}ms", total_ms), width = BAR_WIDTH as usize - 1).dimmed(),
        "",
        lw = label_width,
    );

    for span in spans {
        let source     = span["source"].as_str().unwrap_or("?");
        let resource   = span["resource"].as_str().unwrap_or("?");
        let span_type  = span["span_type"].as_str().unwrap_or("event");
        let delta_ms   = span["delta_ms"].as_i64().unwrap_or(0);
        let elapsed_ms = span["elapsed_ms"].as_i64().unwrap_or(0);
        let is_slow    = span["is_slow"].as_bool().unwrap_or(false);
        let message    = span["message"].as_str().unwrap_or("");

        // Position of the marker in the bar.
        let marker_pos = if total_ms > 0 {
            ((elapsed_ms * BAR_WIDTH) / total_ms).clamp(0, BAR_WIDTH - 1) as usize
        } else {
            0
        };

        // Width of the filled delta block (minimum 1 if non-zero delta).
        let fill_width = if total_ms > 0 && delta_ms > 0 {
            ((delta_ms * BAR_WIDTH) / total_ms).max(1).min(BAR_WIDTH - marker_pos as i64) as usize
        } else {
            1
        };

        // Build three separate string segments (avoids byte-boundary issues with
        // multi-byte block chars) and colourize each independently.
        let prefix_str = format!("|{}", EMPTY.to_string().repeat(marker_pos));
        let fill_str   = FILL.to_string().repeat(fill_width);
        let suffix_len = (BAR_WIDTH as usize).saturating_sub(marker_pos + fill_width);
        let suffix_str = format!("{}|", EMPTY.to_string().repeat(suffix_len));

        let coloured_bar = match span_type {
            "start" => format!("{}{}{}",
                prefix_str.dimmed(), fill_str.cyan().bold(), suffix_str.dimmed()),
            "end"   => format!("{}{}{}",
                prefix_str.dimmed(), fill_str.green().bold(), suffix_str.dimmed()),
            "error" => format!("{}{}{}",
                prefix_str.dimmed(), fill_str.red().bold(), suffix_str.dimmed()),
            _       => format!("{}{}{}",
                prefix_str.dimmed(), fill_str.yellow(), suffix_str.dimmed()),
        };

        // Delta label with slow colouring.
        let delta_label = colorize_delta(delta_ms, slow_thresh, is_slow);

        // Pad plain label then inject colour into source portion.
        let plain_label  = format!("{}/{}", source, resource);
        let padded_plain = format!("{:<width$}", plain_label, width = label_width);
        let coloured_label = padded_plain.replacen(
            source,
            &colorize_source(source).to_string(),
            1,
        );

        // Truncate message so line stays comfortable.
        let short_msg: String = message.chars().take(40).collect();
        let trail = if message.len() > 40 { "…" } else { "" };

        println!(
            "  {}  {}  {}  {}{}",
            coloured_label,
            coloured_bar,
            delta_label,
            short_msg.dimmed(),
            trail.dimmed(),
        );
    }
    println!();
}

// ── Main entry point ──────────────────────────────────────────────────────────

pub async fn execute(request_id: String, slow_threshold: u64, flame: bool) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let url    = format!("{}/traces/{}?slow_ms={}", client.base_url, request_id, slow_threshold);

    let res: reqwest::Response = client.client.get(&url).send().await?;

    if res.status() == reqwest::StatusCode::NOT_FOUND {
        eprintln!("{} no trace found for request ID: {}", "✗".red(), request_id.bold());
        return Ok(());
    }
    if !res.status().is_success() {
        anyhow::bail!("API error: {}", res.status());
    }

    let body: Value = res.json().await?;
    let data         = &body["data"];
    let spans        = data["spans"].as_array().cloned().unwrap_or_default();
    let count        = data["span_count"].as_u64().unwrap_or(spans.len() as u64);
    let total_ms     = data["total_duration_ms"].as_i64();
    let slow_count   = data["slow_span_count"].as_u64().unwrap_or(0);
    let slow_thresh  = data["slow_threshold_ms"].as_u64().unwrap_or(slow_threshold);

    if spans.is_empty() {
        println!("{} No spans found for request ID: {}", "ℹ".blue(), request_id.cyan().bold());
        println!("  Make sure the request was routed through the gateway and logs are enabled.");
        return Ok(());
    }

    // ── Header ────────────────────────────────────────────────────────────────
    println!();
    print!("{} {}", "Trace".bold().white(), request_id.cyan().bold());
    match total_ms {
        Some(t) => print!("  {}ms end-to-end", t.to_string().bold()),
        None    => {},
    }
    println!();
    if slow_count > 0 {
        println!(
            "{}  {} {}",
            "".dimmed(),
            format!("⚠ {} slow (>{}ms)", slow_count, slow_thresh).yellow().bold(),
            format!("— run with --slow {} to adjust", slow_thresh / 2).dimmed(),
        );
    }
    println!("{} spans\n", count.to_string().dimmed());

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

    let mut slow_spans: Vec<(String, String, i64)> = Vec::new(); // (source/resource, message, delta_ms)

    for span in &spans {
        let ts        = span["timestamp"].as_str().unwrap_or("?");
        let source    = span["source"].as_str().unwrap_or("?");
        let resource  = span["resource"].as_str().unwrap_or("?");
        let level     = span["level"].as_str().unwrap_or("info");
        let message   = span["message"].as_str().unwrap_or("");
        let span_type = span["span_type"].as_str().unwrap_or("event");
        let delta_ms  = span["delta_ms"].as_i64().unwrap_or(-1);
        let is_slow   = span["is_slow"].as_bool().unwrap_or(false);

        if is_slow {
            slow_spans.push((format!("{}/{}", source, resource), message.to_string(), delta_ms));
        }

        let delta_col = if delta_ms >= 0 {
            colorize_delta(delta_ms, slow_thresh, is_slow)
        } else {
            "      ".dimmed()
        };

        // Pad the plain label to align columns, then colorise source separately
        let plain_label  = format!("{}/{}", source, resource);
        let padded_plain = format!("{:<width$}", plain_label, width = label_width);
        let coloured_label = padded_plain.replacen(
            source,
            &colorize_source(source).to_string(),
            1,
        );

        println!(
            "  {}  {}  {}  [{}]  {}  {}",
            format_timestamp(ts).dimmed(),
            delta_col,
            colorize_span_icon(span_type, span_icon(span_type)),
            coloured_label,
            colorize_level(level),
            message,
        );
    }

    // ── Footer / flame graph ──────────────────────────────────────────────────
    println!();
    match total_ms {
        Some(t) => println!("  {} spans  •  {}ms total", count, t.to_string().bold()),
        None    => println!("  {} spans", count),
    }

    if !slow_spans.is_empty() {
        println!("\n  {} slow spans (>{}ms):", slow_spans.len().to_string().yellow().bold(), slow_thresh);
        for (label, msg, delta) in &slow_spans {
            println!(
                "    {} {}  {}",
                format!("+{}ms", delta).red().bold(),
                label.bold(),
                msg.dimmed(),
            );
        }
    }

    if flame {
        if let Some(t) = total_ms {
            println!();
            println!("  {}", "Flame graph".bold().white());
            println!();
            render_flame(&spans, t, slow_thresh);
        } else {
            eprintln!("{} flame graph unavailable (total_duration_ms missing from trace)", "⚠".yellow());
        }
    }

    println!();
    Ok(())
}
