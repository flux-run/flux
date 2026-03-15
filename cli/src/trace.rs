use api_contract::routes as R;
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
    let url    = format!("{}?slow_ms={}", R::logs::TRACE_GET.url_with(&client.base_url, &[("request_id", request_id.as_str())]), slow_threshold);

    let res: reqwest::Response = client.client.get(&url).send().await?;

    if res.status() == reqwest::StatusCode::NOT_FOUND {
        eprintln!("{} no trace found for request ID: {}", "✗".red(), request_id.bold());
        return Ok(());
    }
    if !res.status().is_success() {
        anyhow::bail!("API error: {}", res.status());
    }

    let body: Value = res.json().await?;
    // The API returns all fields at the root — no "data" wrapper.
    let spans             = body["spans"].as_array().cloned().unwrap_or_default();
    let count             = body["span_count"].as_u64().unwrap_or(spans.len() as u64);
    let total_ms          = body["total_duration_ms"].as_i64();
    let slow_count        = body["slow_span_count"].as_u64().unwrap_or(0);
    let slow_thresh       = body["slow_threshold_ms"].as_u64().unwrap_or(slow_threshold);
    let slow_db_count     = body["slow_db_count"].as_u64().unwrap_or(0);
    let n_plus_one_tables: Vec<String> = body["n_plus_one_tables"]
        .as_array()
        .map(|a| a.iter().filter_map(|t| t.as_str().map(|s| s.to_string())).collect())
        .unwrap_or_default();
    let suggested_indexes: Vec<serde_json::Value> = body["suggested_indexes"]
        .as_array()
        .cloned()
        .unwrap_or_default();

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
        let ts          = span["timestamp"].as_str().unwrap_or("?");
        let source      = span["source"].as_str().unwrap_or("?");
        let resource    = span["resource"].as_str().unwrap_or("?");
        let level       = span["level"].as_str().unwrap_or("info");
        let message     = span["message"].as_str().unwrap_or("");
        let span_type   = span["span_type"].as_str().unwrap_or("event");
        let delta_ms    = span["delta_ms"].as_i64().unwrap_or(-1);
        let is_slow     = span["is_slow"].as_bool().unwrap_or(false);
        let is_n_plus_1 = span["n_plus_one"].as_bool().unwrap_or(false);

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

        // Append N+1 badge for repeated table queries.
        let display_msg = if is_n_plus_1 {
            format!("{}  {}", message, "⚠ N+1".yellow().bold())
        } else {
            message.to_string()
        };

        println!(
            "  {}  {}  {}  [{}]  {}  {}",
            format_timestamp(ts).dimmed(),
            delta_col,
            colorize_span_icon(span_type, span_icon(span_type)),
            coloured_label,
            colorize_level(level),
            display_msg,
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

    if !n_plus_one_tables.is_empty() {
        println!(
            "\n  {} probable N+1 pattern{}:",
            n_plus_one_tables.len().to_string().yellow().bold(),
            if n_plus_one_tables.len() == 1 { "" } else { "s" },
        );
        for table in &n_plus_one_tables {
            let q_count = spans.iter()
                .filter(|s| s["source"].as_str() == Some("db")
                    && s["metadata"]["table"].as_str() == Some(table.as_str()))
                .count();
            println!(
                "    {} table {} ({} queries)  {}",
                "⚠".yellow().bold(),
                table.bold(),
                q_count,
                "consider batching with IN or preloading all at once".dimmed(),
            );
        }
    }

    if slow_db_count > 0 {
        println!(
            "\n  {} slow db quer{}  {}",
            slow_db_count.to_string().yellow().bold(),
            if slow_db_count == 1 { "y (>50ms)" } else { "ies (>50ms)" },
            "\u{2014} check indexes on the flagged tables".dimmed(),
        );
    }

    if !suggested_indexes.is_empty() {
        println!(
            "\n  {} missing index suggestion{}:",
            suggested_indexes.len().to_string().yellow().bold(),
            if suggested_indexes.len() == 1 { "" } else { "s" },
        );
        for idx in &suggested_indexes {
            if let (Some(table), Some(col), Some(ddl)) = (
                idx["table"].as_str(),
                idx["column"].as_str(),
                idx["ddl"].as_str(),
            ) {
                println!(
                    "    {} {}.{}  run: {}",
                    "\u{2192}".cyan(),
                    table.bold(),
                    col,
                    ddl.green(),
                );
            }
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

// ── flux trace (no ID) — list recent execution records ────────────────────────
//
// Padding must always be applied to plain strings BEFORE colorizing.
// Passing a `colored::ColoredString` into a `{:<N}` width specifier counts
// ANSI escape bytes toward the visible width and produces misaligned columns.

pub async fn execute_list(limit: u64, function: Option<String>, json_output: bool) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    let limit_s = limit.to_string();
    let mut q: Vec<(&str, &str)> = vec![("limit", &limit_s)];
    if let Some(ref fn_name) = function { q.push(("function", fn_name.as_str())); }

    let body: Value = client.get_with(&R::logs::TRACES_LIST, &[], &q).await?;

    if json_output {
        println!("{}", serde_json::to_string_pretty(&body)?);
        return Ok(());
    }

    let empty_vec  = vec![];
    // GET /traces returns { "traces": [...] }
    let records = body["traces"]
        .as_array()
        .unwrap_or(&empty_vec);

    if records.is_empty() {
        println!("{} No traces found.", "ℹ".blue());
        println!("  Invoke a function first: {}", "flux invoke <fn>".cyan());
        return Ok(());
    }

    // Column widths (fixed — no ANSI codes in these strings).
    const W_STATUS:   usize = 9;
    const W_FUNCTION: usize = 26;
    const W_DURATION: usize = 9;
    const W_SPANS:    usize = 6;

    // ── Header ────────────────────────────────────────────────────────────────
    // Pad plain text first, THEN colorize — never the other way around.
    println!();
    println!(
        "  {}  {}  {}  {}  {}",
        format!("{:<w$}", "status",   w = W_STATUS).bold(),
        format!("{:<w$}", "function", w = W_FUNCTION).bold(),
        format!("{:<w$}", "duration", w = W_DURATION).bold(),
        format!("{:<w$}", "spans",    w = W_SPANS).bold(),
        "request_id".dimmed(),
    );
    println!("  {}", "─".repeat(W_STATUS + W_FUNCTION + W_DURATION + W_SPANS + 22).dimmed());

    for record in records {
        let request_id = record["request_id"].as_str()
            .or_else(|| record["id"].as_str())
            .unwrap_or("?");

        let function_name = {
            // list_traces uses "function"; legacy fallbacks for older API versions.
            let raw = record["function"].as_str()
                .or_else(|| record["function_name"].as_str())
                .or_else(|| record["resource"].as_str())
                .unwrap_or("?");
            // Truncate to fit column before padding (avoids ANSI double-count).
            let max = W_FUNCTION - 2;
            if raw.chars().count() > max {
                let truncated: String = raw.chars().take(max).collect();
                format!("{}…", truncated)
            } else {
                raw.to_owned()
            }
        };

        // list_traces returns "status" as an integer HTTP code.
        let http_status = record["status"].as_i64().unwrap_or(0);
        let is_err      = record["is_error"].as_bool().unwrap_or(http_status >= 400);
        let status_raw  = if http_status > 0 { http_status.to_string() } else { "?".to_string() };
        let duration_ms = record["duration_ms"].as_i64()
            .or_else(|| record["total_duration_ms"].as_i64());
        let short_id    = &request_id[..request_id.len().min(8)];

        // Build padded plain strings, then colorize the whole padded cell.
        let status_padded   = format!("{:<w$}", status_raw, w = W_STATUS);
        let function_padded = format!("{:<w$}", function_name, w = W_FUNCTION);
        // spans column not available in list response — omit with dashes.
        let spans_padded    = format!("{:<w$}", "—", w = W_SPANS);

        let status_col = if is_err {
            status_padded.red().bold().to_string()
        } else if http_status >= 300 {
            status_padded.yellow().to_string()
        } else {
            status_padded.green().bold().to_string()
        };

        let duration_plain = match duration_ms {
            Some(d) => format!("{}ms", d),
            None    => "—".to_string(),
        };
        let duration_padded = format!("{:<w$}", duration_plain, w = W_DURATION);
        let duration_col = match duration_ms {
            Some(d) if d >= 1000 => duration_padded.red().bold().to_string(),
            Some(d) if d >= 500  => duration_padded.yellow().to_string(),
            _                    => duration_padded.dimmed().to_string(),
        };

        println!(
            "  {}  {}  {}  {}  {}",
            status_col,
            function_padded.normal(),
            duration_col,
            spans_padded.dimmed(),
            short_id.dimmed(),
        );
    }

    println!();
    println!(
        "  {}",
        format!(
            "{} trace{} — `flux trace <id>` for the full waterfall",
            records.len(),
            if records.len() == 1 { "" } else { "s" },
        ).dimmed(),
    );
    if function.is_none() {
        println!(
            "  {}  {}",
            "tip:".dimmed(),
            "flux trace --function <name>  to filter".cyan(),
        );
    }
    println!();

    Ok(())
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── format_timestamp ──────────────────────────────────────────────────────

    #[test]
    fn format_timestamp_standard_rfc3339() {
        assert_eq!(
            format_timestamp("2026-03-10T10:01:12.031000Z"),
            "10:01:12.031"
        );
    }

    #[test]
    fn format_timestamp_milliseconds_zero_padded() {
        assert_eq!(
            format_timestamp("2026-03-10T09:00:05.005000Z"),
            "09:00:05.005"
        );
    }

    #[test]
    fn format_timestamp_truncates_subsecond_to_3_digits() {
        assert_eq!(
            format_timestamp("2026-01-01T23:59:59.123456Z"),
            "23:59:59.123"
        );
    }

    #[test]
    fn format_timestamp_no_subsecond_pads_ms() {
        let result = format_timestamp("2026-03-10T14:30:00Z");
        assert!(result.starts_with("14:30:00"));
    }

    #[test]
    fn format_timestamp_missing_t_does_not_panic() {
        let result = format_timestamp("invalid");
        assert!(!result.is_empty());
    }

    // ── span_icon ─────────────────────────────────────────────────────────────

    #[test]
    fn span_icon_known_types() {
        assert_eq!(span_icon("start"), "▶");
        assert_eq!(span_icon("end"),   "■");
        assert_eq!(span_icon("error"), "✗");
    }

    #[test]
    fn span_icon_unknown_returns_dot() {
        assert_eq!(span_icon("db"),   "·");
        assert_eq!(span_icon("tool"), "·");
        assert_eq!(span_icon(""),     "·");
    }

    // ── colorize_delta ────────────────────────────────────────────────────────

    #[test]
    fn colorize_delta_does_not_panic() {
        let _ = colorize_delta(0, 500, false);
        let _ = colorize_delta(200, 500, false);
        let _ = colorize_delta(1000, 500, true);
        let _ = colorize_delta(-50, 500, false);
    }
}

