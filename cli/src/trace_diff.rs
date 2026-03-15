//! `flux trace diff <original-id> <replay-id>` — compare two executions of the same request.
//!
//! Shows how behavior diverged between the original production run and a replay:
//! runtime metrics, per-span execution graph diff, and state mutation field diffs.
//!
//! ```text
//! $ flux trace diff 9624a58d a12b4f8c
//!
//! Runtime
//! ────────────────────────────
//! status:        error → success
//! duration:      3816ms → 142ms  (↓96%)
//! errors:        1 → 0
//!
//! Execution Graph
//! ────────────────────────────
//! stripe.charge
//!   original: timeout
//!   replay:   success
//!
//! gmail.send_email
//!   original: executed
//!   replay:   skipped
//!
//! State Diff
//! ────────────────────────────
//! users.id=42
//!
//!   plan
//!     original: free → pro
//!     replay:   free → enterprise
//!
//! Verdict
//! ────────────────────────────
//! FIXED
//! ```

use std::collections::VecDeque;
use colored::Colorize;
use serde_json::Value;

use api_contract::routes as R;
use crate::client::ApiClient;
use crate::why::diff_json;

/// Page size for cursor-paginated mutation fetches.
/// Peak memory for State Diff = 2 × PAGE_SIZE × avg-row-size.
/// At 3 KB/row: 2 × 1000 × 3 KB = 6 MB regardless of total mutation count.
const PAGE_SIZE: u32 = 1000;

// ── Entry point ──────────────────────────────────────────────────────────────

pub async fn execute(
    original_id: String,
    replay_id:   String,
    json_output: bool,
    table:       Option<String>,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    // Traces are small (one JSON object per request) — fetch both eagerly.
    let orig_trace_url = format!("{}?slow_ms=0", R::logs::TRACE_GET.url_with(&client.base_url, &[("request_id", original_id.as_str())]));
    let rep_trace_url  = format!("{}?slow_ms=0", R::logs::TRACE_GET.url_with(&client.base_url, &[("request_id", replay_id.as_str())]));

    let (orig_trace, rep_trace) = tokio::try_join!(
        fetch_json(&client, &orig_trace_url),
        fetch_json(&client, &rep_trace_url),
    )?;

    if json_output {
        // For JSON output we still stream-paginate mutations but collect them all
        // so the output is a single valid JSON document.
        let (orig_muts, rep_muts) = tokio::try_join!(
            collect_all_mutations(&client, &original_id, table.as_deref()),
            collect_all_mutations(&client, &replay_id,   table.as_deref()),
        )?;
        println!("{}", serde_json::to_string_pretty(&serde_json::json!({
            "original": { "trace": orig_trace, "mutations": orig_muts },
            "replay":   { "trace": rep_trace,  "mutations": rep_muts  },
        }))?);
        return Ok(());
    }

    // ── Extract metadata ─────────────────────────────────────────────────────
    let orig_req = orig_trace.get("request").unwrap_or(&Value::Null);
    let rep_req  = rep_trace .get("request").unwrap_or(&Value::Null);

    let method   = orig_req["method"].as_str().unwrap_or("?");
    let path     = orig_req["path"].as_str().unwrap_or("?");
    let function = orig_req["function"].as_str().unwrap_or("?");

    let orig_commit = short_sha(orig_req["code_sha"].as_str().unwrap_or("?"));
    let rep_commit  = short_sha(rep_req ["code_sha"].as_str().unwrap_or("?"));

    let orig_status  = orig_req["status"].as_i64().unwrap_or(0);
    let rep_status   = rep_req ["status"].as_i64().unwrap_or(0);

    let orig_ms = orig_trace["total_ms"].as_i64().unwrap_or(0);
    let rep_ms  = rep_trace ["total_ms"].as_i64().unwrap_or(0);

    let empty_spans: Vec<Value> = vec![];
    let orig_spans_all = orig_trace["spans"].as_array().unwrap_or(&empty_spans);
    let rep_spans_all  = rep_trace["spans"].as_array().unwrap_or(&empty_spans);

    let orig_errors = orig_spans_all.iter().filter(|s| s["span_type"] == "error").count();
    let rep_errors  = rep_spans_all .iter().filter(|s| s["span_type"] == "error").count();

    // ── Print header ─────────────────────────────────────────────────────────
    println!();
    println!("  {}  {} {} → {}", "endpoint:".dimmed(), method.bold(), path.bold(), function.cyan().bold());
    println!("  {}  {}  commit {}", "original:".dimmed(), trunc(&original_id, 12).yellow(), orig_commit.dimmed());
    println!("  {}  {}  commit {}", "replay:  ".dimmed(), trunc(&replay_id, 12).cyan(),    rep_commit.dimmed());
    if let Some(ref t) = table {
        println!("  {}  {}", "filter:  ".dimmed(), format!("table={t}").cyan());
    }
    println!();

    // ── Runtime section ──────────────────────────────────────────────────────
    println!("{}", "Runtime".bold());
    println!("{}", "─".repeat(28).dimmed());

    // Status
    let status_changed = orig_status != rep_status;
    let status_icon = if !status_changed {
        "═".dimmed().to_string()
    } else if is_error(rep_status) {
        "✗".red().bold().to_string()
    } else {
        "✔".green().bold().to_string()
    };
    let verdict_label = if !status_changed {
        "(same)".dimmed().to_string()
    } else if is_error(orig_status) && !is_error(rep_status) {
        "fixed".green().bold().to_string()
    } else if !is_error(orig_status) && is_error(rep_status) {
        "regressed".red().bold().to_string()
    } else {
        "changed".yellow().to_string()
    };
    println!("  {}  {} → {}  {} {}",
        "status:  ".dimmed(),
        format_status(orig_status).yellow(),
        format_status(rep_status).cyan(),
        status_icon,
        verdict_label,
    );

    // Duration
    let ms_pct = if orig_ms > 0 {
        let delta = rep_ms - orig_ms;
        let pct   = (delta * 100) / orig_ms;
        if delta < 0 { format!("(↓{}%)", -pct).green().to_string() }
        else if delta == 0 { "(same)".dimmed().to_string() }
        else               { format!("(↑{}%)", pct).red().to_string() }
    } else {
        String::new()
    };
    println!("  {}  {}ms → {}ms  {}",
        "duration:".dimmed(),
        orig_ms.to_string().yellow(),
        rep_ms.to_string().cyan(),
        ms_pct,
    );

    // Errors
    let err_icon = if orig_errors > rep_errors {
        format!("↓ {}", orig_errors - rep_errors).green().to_string()
    } else if rep_errors > orig_errors {
        format!("↑ {}", rep_errors - orig_errors).red().to_string()
    } else {
        "(same)".dimmed().to_string()
    };
    println!("  {}  {} → {}  {}",
        "errors:  ".dimmed(),
        orig_errors.to_string().yellow(),
        rep_errors.to_string().cyan(),
        err_icon,
    );
    println!();

    // ── Execution Graph section ───────────────────────────────────────────────
    let span_diffs = diff_spans(orig_spans_all, rep_spans_all);
    let any_span_diff = span_diffs.iter().any(|d| d.changed);

    if any_span_diff {
        println!("{}", "Execution Graph".bold());
        println!("{}", "─".repeat(28).dimmed());

        for sd in span_diffs.iter().filter(|d| d.changed) {
            println!();
            println!("  {}", sd.name.cyan().bold());
            match (&sd.orig_status, &sd.rep_status) {
                (Some(o), Some(r)) => {
                    println!("  {}  {}", "  original:".dimmed(), color_status_label(o));
                    println!("  {}  {}", "  replay:  ".dimmed(), color_status_label(r));
                }
                (Some(o), None) => {
                    println!("  {}  {}", "  original:".dimmed(), color_status_label(o));
                    println!("  {}  {}", "  replay:  ".dimmed(), "skipped".dimmed());
                }
                (None, Some(r)) => {
                    println!("  {}  {}", "  original:".dimmed(), "skipped".dimmed());
                    println!("  {}  {}", "  replay:  ".dimmed(), color_status_label(r));
                }
                (None, None) => {}
            }
            // Duration change, if both ran and differed meaningfully
            if let (Some(od), Some(rd)) = (sd.orig_ms, sd.rep_ms) {
                if od > 0 {
                    let abs_diff = (rd - od).abs();
                    let delta_pct = (abs_diff * 100) / od;
                    if abs_diff >= 100 || delta_pct >= 20 {
                        let arrow = if delta_pct < 0 {
                            format!("↓{}%", -delta_pct).green().to_string()
                        } else {
                            format!("↑{}%", delta_pct).red().to_string()
                        };
                        println!("  {}  {}ms → {}ms  {}",
                            "  duration:".dimmed(), od, rd, arrow);
                    }
                }
            }
        }
        println!();
    }

    // ── State Diff — streaming paginated ─────────────────────────────────────
    //
    // Instead of fetching all mutations into memory (O(n)), we interleave two
    // page-cursors and compare mutations pair-by-pair in relative position.
    // Peak memory = 2 × PAGE_SIZE rows regardless of total mutation count.
    // For 100k mutations @ 3 KB/row that's 6 MB vs ~300 MB with a single fetch.
    println!("{}", "State Diff".bold());
    println!("{}", "─".repeat(28).dimmed());

    let mut orig_iter = MutIter::new(&client, &original_id, table.as_deref());
    let mut rep_iter  = MutIter::new(&client, &replay_id,   table.as_deref());

    let mut mutation_count  = 0usize;
    let mut state_changed   = false;

    loop {
        let om = orig_iter.next().await?;
        let rm = rep_iter .next().await?;

        match (om, rm) {
            (None, None) => break,

            (Some(o), Some(r)) => {
                mutation_count += 1;
                let table_name = o["table_name"].as_str().unwrap_or("?");
                let op         = o["operation"].as_str().unwrap_or("?");
                let ver        = o["mutation_seq"].as_i64()
                    .or_else(|| o["version"].as_i64()).unwrap_or(0);

                let mutations_match = o["after_state"] == r["after_state"]
                    && o["before_state"] == r["before_state"];

                if !mutations_match { state_changed = true; }

                let row_id = o["after_state"].get("id")
                    .or_else(|| o["before_state"].get("id"))
                    .and_then(|v| v.as_str().map(|s| trunc(s, 8))
                        .or_else(|| v.as_i64().map(|n| n.to_string())))
                    .unwrap_or_default();

                let same_label = if mutations_match { "  (same)".dimmed().to_string() } else { String::new() };

                println!();
                println!("  {}.{}{}  {}{}",
                    table_name.cyan(),
                    if row_id.is_empty() { format!("seq{ver}") } else { format!("id={row_id}") },
                    String::new(),
                    color_op(op),
                    same_label,
                );

                if !mutations_match && op == "update" {
                    let o_diffs = diff_json(&o["before_state"], &o["after_state"]);
                    let r_diffs = diff_json(&r["before_state"], &r["after_state"]);

                    let mut keys: Vec<String> = o_diffs.iter().map(|(k,_,_)| k.clone())
                        .chain(r_diffs.iter().map(|(k,_,_)| k.clone()))
                        .collect();
                    keys.dedup();

                    for key in keys {
                        let o_change = o_diffs.iter().find(|(k,_,_)| k == &key);
                        let r_change = r_diffs.iter().find(|(k,_,_)| k == &key);

                        match (o_change, r_change) {
                            (Some((_, ob, oa)), Some((_, _rb, ra))) if oa == ra => {
                                println!("    {}  {} → {}  (same)",
                                    format!("{key}:").dimmed(),
                                    ob.yellow(), ra.green());
                            }
                            (Some((_, ob, oa)), Some((_, rb, ra))) => {
                                println!();
                                println!("    {}", key.bold());
                                println!("    {}  {} → {}",
                                    "  original:".dimmed(), ob.yellow(), oa.red());
                                println!("    {}  {} → {}",
                                    "  replay:  ".dimmed(), rb.yellow(), ra.green().bold());
                            }
                            (Some((_, ob, oa)), None) => {
                                println!();
                                println!("    {}", key.bold());
                                println!("    {}  {} → {}",
                                    "  original:".dimmed(), ob.yellow(), oa.red());
                                println!("    {}  {}", "  replay:  ".dimmed(), "(no change)".dimmed());
                            }
                            (None, Some((_, _, ra))) => {
                                println!();
                                println!("    {}", key.bold());
                                println!("    {}  {}", "  original:".dimmed(), "(no change)".dimmed());
                                println!("    {}  → {}", "  replay:  ".dimmed(), ra.green().bold());
                            }
                            _ => {}
                        }
                    }
                } else if !mutations_match && op == "insert" {
                    let diffs = diff_json(&r["after_state"], &o["after_state"]);
                    for (key, rval, oval) in diffs.iter().take(5) {
                        println!("    {}  original: {}   replay: {}",
                            format!("{key}:").dimmed(), oval.yellow(), rval.cyan());
                    }
                }
            }

            (Some(o), None) => {
                mutation_count += 1;
                state_changed = true;
                let table_name = o["table_name"].as_str().unwrap_or("?");
                let op         = o["operation"].as_str().unwrap_or("?");
                println!();
                println!("  {}  {}  replay: {}",
                    table_name.cyan(), color_op(op), "missing".red());
                // drain remaining original mutations without re-allocating
                while orig_iter.next().await?.is_some() {
                    mutation_count += 1;
                }
                break;
            }

            (None, Some(r)) => {
                mutation_count += 1;
                state_changed = true;
                let table_name = r["table_name"].as_str().unwrap_or("?");
                let op         = r["operation"].as_str().unwrap_or("?");
                println!();
                println!("  {}  {}  original: {}",
                    table_name.cyan(), color_op(op), "missing".red());
                while rep_iter.next().await?.is_some() {
                    mutation_count += 1;
                }
                break;
            }
        }
    }

    if mutation_count == 0 {
        println!("  {}", "no mutations in either execution".dimmed());
    }

    println!();

    // ── Verdict ──────────────────────────────────────────────────────────────
    println!("{}", "Verdict".bold());
    println!("{}", "─".repeat(28).dimmed());

    let behavior_changed = status_changed || orig_errors != rep_errors || state_changed || any_span_diff;

    if behavior_changed {
        let (icon, label) = if !is_error(rep_status) && is_error(orig_status) {
            ("✔".green().bold().to_string(), "FIXED".green().bold().to_string())
        } else if is_error(rep_status) && !is_error(orig_status) {
            ("✗".red().bold().to_string(), "REGRESSED".red().bold().to_string())
        } else {
            ("≠".yellow().bold().to_string(), "BEHAVIOR CHANGED".yellow().bold().to_string())
        };
        println!("  {} {}", icon, label);
    } else {
        println!("  {} {}", "═".dimmed(), "IDENTICAL".dimmed());
    }

    println!();
    Ok(())
}

// ── Streaming mutation cursor ─────────────────────────────────────────────────

/// Async cursor that pages through a request's mutations using keyset pagination.
/// Holds at most PAGE_SIZE rows in memory at any time.
struct MutIter<'a> {
    client:     &'a ApiClient,
    request_id: &'a str,
    table:      Option<&'a str>,
    buffer:     VecDeque<Value>,
    after_seq:  Option<i64>,   // None = start from beginning
    exhausted:  bool,
}

impl<'a> MutIter<'a> {
    fn new(client: &'a ApiClient, request_id: &'a str, table: Option<&'a str>) -> Self {
        Self {
            client,
            request_id,
            table,
            buffer: VecDeque::new(),
            after_seq: None,
            exhausted: false,
        }
    }

    /// Return the next mutation row, fetching a new page when the buffer is empty.
    /// Returns `None` when the log is fully consumed.
    async fn next(&mut self) -> anyhow::Result<Option<Value>> {
        if self.buffer.is_empty() {
            if self.exhausted {
                return Ok(None);
            }
            self.fill_buffer().await?;
        }
        Ok(self.buffer.pop_front())
    }

    async fn fill_buffer(&mut self) -> anyhow::Result<()> {
        let page = fetch_mutations_page(
            self.client,
            self.request_id,
            PAGE_SIZE,
            self.after_seq,
            self.table,
        ).await?;

        let next_cursor = page["next_after_seq"].as_i64();
        self.exhausted  = next_cursor.is_none();
        self.after_seq  = next_cursor;

        if let Some(arr) = page["mutations"].as_array() {
            self.buffer.extend(arr.iter().cloned());
        } else {
            self.exhausted = true;
        }
        Ok(())
    }
}

/// Fetch one page of mutations from the data-engine REST API.
async fn fetch_mutations_page(
    client:     &ApiClient,
    request_id: &str,
    limit:      u32,
    after_seq:  Option<i64>,
    table:      Option<&str>,
) -> anyhow::Result<Value> {
    let mut url = format!(
        "{}/db/mutations?request_id={}&limit={}",
        client.base_url, request_id, limit,
    );
    if let Some(seq) = after_seq {
        url.push_str(&format!("&after_seq={seq}"));
    }
    if let Some(t) = table {
        url.push_str(&format!("&table_name={t}"));
    }
    fetch_json(client, &url).await
}

/// Collect all mutations across pages (used only for `--json` output).
async fn collect_all_mutations(
    client:     &ApiClient,
    request_id: &str,
    table:      Option<&str>,
) -> anyhow::Result<Vec<Value>> {
    let mut all: Vec<Value> = Vec::new();
    let mut after_seq: Option<i64> = None;
    loop {
        let page = fetch_mutations_page(client, request_id, PAGE_SIZE, after_seq, table).await?;
        let next = page["next_after_seq"].as_i64();
        if let Some(arr) = page["mutations"].as_array() {
            all.extend(arr.iter().cloned());
        }
        match next {
            Some(c) => after_seq = Some(c),
            None    => break,
        }
    }
    Ok(all)
}

// ── Span diff ────────────────────────────────────────────────────────────────

/// Summary of a single span's observable behaviour across one execution.
#[derive(Debug, Default)]
struct SpanEntry {
    status: Option<String>,
    ms:     Option<i64>,
}

/// One row in the execution-graph diff table.
struct SpanDiff {
    name:        String,
    orig_status: Option<String>,
    rep_status:  Option<String>,
    orig_ms:     Option<i64>,
    rep_ms:      Option<i64>,
    /// True when the two executions differ in a user-visible way.
    changed:     bool,
}

/// Compare the span arrays from two traces.
/// Returns one `SpanDiff` per span name that appears in at least one trace,
/// ordered by first appearance in the original.
fn diff_spans(orig: &[Value], rep: &[Value]) -> Vec<SpanDiff> {
    fn relevant(s: &Value) -> bool {
        matches!(
            s["span_type"].as_str().unwrap_or(""),
            "tool" | "db" | "workflow_step" | "agent_step"
        )
    }

    fn span_key(s: &Value) -> String {
        let kind = s["span_type"].as_str().unwrap_or("span");
        let name = s["data"]["action"].as_str()
            .or_else(|| s["data"]["tool"].as_str())
            .or_else(|| s["data"]["table"].as_str())
            .or_else(|| s["message"].as_str())
            .or_else(|| s["name"].as_str())
            .unwrap_or(kind);
        name.to_string()
    }

    fn span_status(s: &Value) -> Option<String> {
        s["status"].as_str()
            .or_else(|| if s["span_type"] == "error" { Some("error") } else { None })
            .or_else(|| if s["data"]["success"].as_bool() == Some(false) { Some("error") } else { None })
            .map(|s| s.to_string())
            .or_else(|| Some("executed".to_string()))
    }

    fn collect(spans: &[Value]) -> Vec<(String, SpanEntry)> {
        let mut vec: Vec<(String, SpanEntry)> = Vec::new();
        for s in spans.iter().filter(|s| relevant(s)) {
            let key = span_key(s);
            if let Some((_, e)) = vec.iter_mut().find(|(k, _)| k == &key) {
                e.status = span_status(s);
                e.ms = s["duration_ms"].as_i64().or(e.ms);
            } else {
                vec.push((key, SpanEntry {
                    status: span_status(s),
                    ms:     s["duration_ms"].as_i64(),
                }));
            }
        }
        vec
    }

    let orig_vec = collect(orig);
    let rep_vec  = collect(rep);

    let mut all_keys: Vec<String> = orig_vec.iter().map(|(k, _)| k.clone()).collect();
    for (k, _) in &rep_vec {
        if !all_keys.contains(k) {
            all_keys.push(k.clone());
        }
    }

    all_keys.into_iter().map(|name| {
        let oe = orig_vec.iter().find(|(k, _)| k == &name).map(|(_, e)| e);
        let re = rep_vec .iter().find(|(k, _)| k == &name).map(|(_, e)| e);

        let orig_status = oe.and_then(|e| e.status.clone());
        let rep_status  = re.and_then(|e| e.status.clone());
        let orig_ms     = oe.and_then(|e| e.ms);
        let rep_ms      = re.and_then(|e| e.ms);

        let status_diff = orig_status != rep_status || oe.is_none() != re.is_none();
        let dur_diff = match (orig_ms, rep_ms) {
            (Some(o), Some(r)) if o > 0 => {
                let abs_diff = (r - o).abs();
                let pct_diff = (abs_diff * 100) / o;
                abs_diff >= 100 || pct_diff >= 20
            }
            _ => false,
        };

        SpanDiff {
            name,
            orig_status,
            rep_status,
            orig_ms,
            rep_ms,
            changed: status_diff || dur_diff,
        }
    }).collect()
}

// ── Helpers ──────────────────────────────────────────────────────────────────

async fn fetch_json(client: &ApiClient, url: &str) -> anyhow::Result<Value> {
    let res = client.client.get(url).send().await?;
    if res.status().is_success() {
        Ok(res.json().await.unwrap_or_default())
    } else {
        Ok(Value::Null)
    }
}

fn color_status_label(s: &str) -> colored::ColoredString {
    match s {
        "error" | "timeout" | "failed" => s.red().bold(),
        "success" | "executed"         => s.green(),
        "skipped"                      => s.dimmed(),
        _                              => s.normal(),
    }
}

fn trunc(s: &str, n: usize) -> String {
    if s.len() > n { format!("{}…", &s[..n]) } else { s.to_string() }
}

fn short_sha(sha: &str) -> String {
    if sha.len() >= 7 { sha[..7].to_string() } else { sha.to_string() }
}

fn is_error(status: i64) -> bool { status >= 400 || status == 0 }

fn format_status(status: i64) -> String {
    if status == 0 { "?".to_string() }
    else if status < 400 { format!("{status} OK") }
    else { format!("{status} FAILED") }
}

fn color_op(op: &str) -> colored::ColoredString {
    match op {
        "insert" => op.green().bold(),
        "update" => op.yellow().bold(),
        "delete" => op.red().bold(),
        _        => op.normal(),
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── trunc ─────────────────────────────────────────────────────────────────

    #[test]
    fn trunc_short_string_unchanged() {
        assert_eq!(trunc("hi", 10), "hi");
    }

    #[test]
    fn trunc_exact_length_unchanged() {
        assert_eq!(trunc("hello", 5), "hello");
    }

    #[test]
    fn trunc_long_string_truncated_with_ellipsis() {
        let result = trunc("hello world", 5);
        assert!(result.starts_with("hello"));
        assert!(result.contains('…'));
        assert!(result.len() <= 10); // 5 chars + 3-byte '…'
    }

    #[test]
    fn trunc_empty_string() {
        assert_eq!(trunc("", 5), "");
    }

    // ── short_sha ─────────────────────────────────────────────────────────────

    #[test]
    fn short_sha_long_hash_truncated_to_7() {
        assert_eq!(short_sha("abcdef1234567890"), "abcdef1");
    }

    #[test]
    fn short_sha_exactly_7_chars_unchanged() {
        assert_eq!(short_sha("abcdef1"), "abcdef1");
    }

    #[test]
    fn short_sha_short_hash_returned_as_is() {
        assert_eq!(short_sha("abc"), "abc");
    }

    // ── is_error ──────────────────────────────────────────────────────────────

    #[test]
    fn is_error_zero_is_error() {
        assert!(is_error(0));
    }

    #[test]
    fn is_error_400_is_error() {
        assert!(is_error(400));
    }

    #[test]
    fn is_error_500_is_error() {
        assert!(is_error(500));
    }

    #[test]
    fn is_error_200_not_error() {
        assert!(!is_error(200));
    }

    #[test]
    fn is_error_399_not_error() {
        assert!(!is_error(399));
    }

    // ── format_status ─────────────────────────────────────────────────────────

    #[test]
    fn format_status_zero_is_question_mark() {
        assert_eq!(format_status(0), "?");
    }

    #[test]
    fn format_status_200_ok() {
        assert_eq!(format_status(200), "200 OK");
    }

    #[test]
    fn format_status_201_ok() {
        assert_eq!(format_status(201), "201 OK");
    }

    #[test]
    fn format_status_400_failed() {
        assert_eq!(format_status(400), "400 FAILED");
    }

    #[test]
    fn format_status_500_failed() {
        assert_eq!(format_status(500), "500 FAILED");
    }

    // ── diff_spans ────────────────────────────────────────────────────────────

    fn make_span(span_type: &str, name: &str, status: &str, ms: i64) -> Value {
        json!({
            "span_type":   span_type,
            "name":        name,
            "message":     name,
            "status":      status,
            "duration_ms": ms
        })
    }

    fn make_tool_span(action: &str, status: &str, ms: i64) -> Value {
        json!({
            "span_type":   "tool",
            "data":        { "action": action },
            "status":      status,
            "duration_ms": ms
        })
    }

    fn make_db_span(table: &str, status: &str, ms: i64) -> Value {
        json!({
            "span_type":   "db",
            "data":        { "table": table },
            "status":      status,
            "duration_ms": ms
        })
    }

    #[test]
    fn diff_spans_identical_traces_not_changed() {
        let orig = vec![make_tool_span("search", "executed", 50)];
        let rep  = vec![make_tool_span("search", "executed", 55)];
        let diffs = diff_spans(&orig, &rep);
        assert_eq!(diffs.len(), 1);
        // ~10% change — below the 20% threshold
        assert!(!diffs[0].changed);
    }

    #[test]
    fn diff_spans_status_change_is_changed() {
        let orig = vec![make_tool_span("search", "executed", 50)];
        let rep  = vec![make_tool_span("search", "error",    50)];
        let diffs = diff_spans(&orig, &rep);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].changed);
        assert_eq!(diffs[0].orig_status.as_deref(), Some("executed"));
        assert_eq!(diffs[0].rep_status.as_deref(),  Some("error"));
    }

    #[test]
    fn diff_spans_missing_in_replay_is_changed() {
        let orig = vec![make_tool_span("search", "executed", 50)];
        let rep: Vec<Value> = vec![];
        let diffs = diff_spans(&orig, &rep);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].changed);
        assert!(diffs[0].rep_status.is_none());
    }

    #[test]
    fn diff_spans_new_in_replay_is_changed() {
        let orig: Vec<Value> = vec![];
        let rep = vec![make_tool_span("email", "executed", 100)];
        let diffs = diff_spans(&orig, &rep);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].changed);
        assert!(diffs[0].orig_status.is_none());
    }

    #[test]
    fn diff_spans_large_duration_change_is_flagged() {
        // original 100ms, replay 200ms → 100% change → flagged
        let orig = vec![make_db_span("users", "executed", 100)];
        let rep  = vec![make_db_span("users", "executed", 200)];
        let diffs = diff_spans(&orig, &rep);
        assert_eq!(diffs.len(), 1);
        assert!(diffs[0].changed);
    }

    #[test]
    fn diff_spans_non_relevant_span_types_ignored() {
        // "start", "end", "log" are not in the relevant set — should produce no diffs
        let orig = vec![
            make_span("start", "req",  "executed", 0),
            make_span("end",   "req",  "executed", 0),
            make_span("log",   "info", "executed", 0),
        ];
        let rep = orig.clone();
        let diffs = diff_spans(&orig, &rep);
        assert_eq!(diffs.len(), 0);
    }

    #[test]
    fn diff_spans_multiple_spans_all_stable() {
        let orig = vec![
            make_tool_span("search", "executed", 50),
            make_db_span("users",   "executed", 10),
        ];
        let rep = vec![
            make_tool_span("search", "executed", 52),
            make_db_span("users",   "executed", 11),
        ];
        let diffs = diff_spans(&orig, &rep);
        assert_eq!(diffs.len(), 2);
        assert!(diffs.iter().all(|d| !d.changed));
    }

    #[test]
    fn diff_spans_preserves_original_ordering() {
        let orig = vec![
            make_tool_span("alpha", "executed", 10),
            make_tool_span("beta",  "executed", 20),
        ];
        let rep = vec![
            make_tool_span("beta",  "executed", 20),
            make_tool_span("alpha", "executed", 10),
        ];
        let diffs = diff_spans(&orig, &rep);
        // Order follows orig: alpha first, beta second
        assert_eq!(diffs[0].name, "alpha");
        assert_eq!(diffs[1].name, "beta");
    }
}

