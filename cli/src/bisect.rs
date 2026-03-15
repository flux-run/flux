//! `flux bug bisect` — binary search commits to find the first regression.
//!
//! Builds a commit timeline from trace history, then binary-searches between
//! a known-good and known-bad commit SHA to locate the exact commit that
//! introduced the failure — without requiring any replays or deploys.
//!
//! ```text
//! $ flux bug bisect --function create_user --good a93f42c --bad 9624a58d
//!
//! ─── Bisecting create_user ────────────────────────────────────────
//!
//!   timeline: 8 deployments, good=a93f42c..bad=9624a58d (4 to search)
//!
//!   [1/2]  testing 3b21aaf  →  ✔ GOOD  (0/12 requests failed)
//!   [2/2]  testing c72de1b  →  ✗ BAD   (5/9 requests failed)
//!
//! ─── Result ──────────────────────────────────────────────────────
//!
//!   first bad commit:  c72de1b
//!
//!   ✗ BAD   c72de1b  2026-03-11T14:22:00Z  (5/9 failed)
//!   ✔ GOOD  3b21aaf  2026-03-11T12:11:00Z  (0/12 failed)
//!
//!   run:  flux trace <request-id>   to inspect a failing request
//!         flux why  <request-id>    for root cause
//! ```

use colored::Colorize;
use serde_json::Value;

use api_contract::routes as R;
use crate::client::ApiClient;

// ── Entry point ──────────────────────────────────────────────────────────────

pub async fn execute(
    function:    String,
    good_commit: String,
    bad_commit:  String,
    threshold:   f64,
    json_output: bool,
) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    println!();
    println!(
        "{}{}{}{}",
        "─── Bisecting ".bold(),
        function.cyan().bold(),
        "  ".bold(),
        "────────────────────────────────────────────────────────".dimmed(),
    );
    println!();

    // ── Build commit timeline ────────────────────────────────────────────────
    eprintln!("{}", "  fetching trace history…".dimmed());

    let body: Value = client
        .get_with(&R::logs::TRACES_LIST, &[], &[("function", function.as_str()), ("limit", "500")])
        .await
        .unwrap_or(Value::Null);

    let empty_arr = vec![];
    let traces: &Vec<Value> = body.get("traces")
        .or_else(|| body.as_array().map(|_| &body).and(Some(&body).filter(|_| false)))
        .and_then(|v| v.as_array())
        .unwrap_or(&empty_arr);

    // ── Build per-commit statistics ──────────────────────────────────────────
    // Group traces by code_sha, preserving chronological order.
    let mut commits: Vec<CommitData> = Vec::new();

    for tr in traces {
        let sha   = tr["code_sha"].as_str().unwrap_or("").to_string();
        let ts    = tr["created_at"].as_str()
            .or_else(|| tr["started_at"].as_str())
            .unwrap_or("")
            .to_string();
        let fail  = tr["status"].as_i64().unwrap_or(0) >= 400
            || tr["spans"].as_array().map(|s| s.iter().any(|sp| sp["span_type"] == "error")).unwrap_or(false);
        let req_id = tr["request_id"].as_str().unwrap_or("").to_string();

        if sha.is_empty() { continue; }

        if let Some(c) = commits.iter_mut().find(|c| c.sha == sha) {
            c.total += 1;
            if fail { c.failed += 1; c.sample_request_id.get_or_insert(req_id); }
        } else {
            commits.push(CommitData {
                sha:               sha,
                first_seen:        ts,
                total:             1,
                failed:            if fail { 1 } else { 0 },
                sample_request_id: if fail { Some(req_id) } else { None },
            });
        }
    }

    if commits.is_empty() {
        eprintln!("{}", "  no trace data found for this function. Have requests been made?".yellow());
        return Ok(());
    }

    // ── Sort by first_seen ascending (oldest → newest) ───────────────────────
    commits.sort_by(|a, b| a.first_seen.cmp(&b.first_seen));

    // ── Find good and bad indices ────────────────────────────────────────────
    let good_idx = commits.iter()
        .position(|c| c.sha.starts_with(&good_commit) || good_commit.starts_with(&c.sha));
    let bad_idx  = commits.iter()
        .position(|c| c.sha.starts_with(&bad_commit)  || bad_commit.starts_with(&c.sha));

    let good_idx = match good_idx {
        Some(i) => i,
        None => {
            eprintln!("{}", format!("  commit {} not found in trace history.", good_commit).red());
            eprintln!("{}", "  known commits:".dimmed());
            for c in commits.iter().take(10) {
                eprintln!("    {}", short_sha(&c.sha).dimmed());
            }
            return Ok(());
        }
    };
    let bad_idx = match bad_idx {
        Some(i) => i,
        None => {
            eprintln!("{}", format!("  commit {} not found in trace history.", bad_commit).red());
            return Ok(());
        }
    };

    if good_idx >= bad_idx {
        eprintln!("{}", "  --good commit must appear earlier in history than --bad commit.".red());
        return Ok(());
    }

    let search_range = &commits[good_idx..=bad_idx];
    println!(
        "  timeline: {} deployments,  good={}..bad={}  ({} to search)",
        commits.len(),
        short_sha(&good_commit).green(),
        short_sha(&bad_commit).red(),
        search_range.len() - 2,  // exclude the endpoints themselves
    );
    println!();

    if search_range.len() <= 2 {
        println!("{}", "  nothing to bisect — good and bad are adjacent commits.".yellow());
        println!();
        show_result(search_range.last().unwrap(), search_range.first().unwrap(), json_output);
        return Ok(());
    }

    // ── Binary search ────────────────────────────────────────────────────────
    let inner = &search_range[1..search_range.len() - 1]; // exclude known endpoints
    let total_steps = (inner.len() as f64).log2().ceil() as usize + 1;
    let mut lo = 0usize;
    let mut hi = inner.len();
    let mut step = 0usize;

    // is_bad returns true when the failure rate exceeds threshold
    let is_bad = |c: &CommitData| -> bool {
        if c.total == 0 { return false; }
        (c.failed as f64 / c.total as f64) >= threshold
    };

    let mut first_bad: Option<usize> = None; // index into `inner`

    while lo < hi {
        let mid = (lo + hi) / 2;
        step += 1;
        let c = &inner[mid];

        let bad = is_bad(c);
        let result_label = if bad {
            format!("✗ BAD   ({}/{} failed)", c.failed, c.total).red().bold().to_string()
        } else {
            format!("✔ GOOD  ({}/{} failed)", c.failed, c.total).green().to_string()
        };

        print!(
            "  [{}/~{}]  testing {}  {}  ",
            step, total_steps,
            short_sha(&c.sha).bold(),
            c.first_seen.get(..16).unwrap_or(&c.first_seen).dimmed(),
        );
        println!("→  {}", result_label);

        if bad {
            first_bad = Some(mid);
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }

    println!();

    // ── Result ───────────────────────────────────────────────────────────────
    println!("{}", "─── Result ──────────────────────────────────────────────────────────────────".bold());
    println!();

    let first_bad_commit = first_bad
        .map(|i| &inner[i])
        .or_else(|| search_range.last()); // fallback: known bad endpoint

    // Last known good = commit just before first_bad, or good endpoint
    let last_good_commit = first_bad
        .and_then(|i| if i > 0 { Some(&inner[i - 1]) } else { Some(&search_range[0]) })
        .or(Some(&search_range[0]));

    match (first_bad_commit, last_good_commit) {
        (Some(bad), Some(good)) => {
            println!("  {} {}  {}", "first bad commit:".bold(), short_sha(&bad.sha).red().bold(), bad.first_seen.get(..19).unwrap_or("").dimmed());
            println!();
            println!("  {}  {}  {}  ({}/{} failed)",
                "✗ BAD ".red().bold(),
                short_sha(&bad.sha).red(),
                bad.first_seen.get(..19).unwrap_or("").dimmed(),
                bad.failed, bad.total,
            );
            println!("  {}  {}  {}  ({}/{} failed)",
                "✔ GOOD".green(),
                short_sha(&good.sha).green(),
                good.first_seen.get(..19).unwrap_or("").dimmed(),
                good.failed, good.total,
            );

            if let Some(rid) = &bad.sample_request_id {
                println!();
                let ridshort = trunc(rid, 12);
                println!("  {}  {} {}",
                    "sample failing request:".dimmed(),
                    ridshort.yellow(),
                    format!("(flux why {ridshort})").dimmed(),
                );
            }

            if json_output {
                let out = serde_json::json!({
                    "first_bad": { "sha": bad.sha, "failed": bad.failed, "total": bad.total, "first_seen": bad.first_seen },
                    "last_good": { "sha": good.sha, "failed": good.failed, "total": good.total, "first_seen": good.first_seen },
                    "sample_request_id": bad.sample_request_id,
                });
                println!("\n{}", serde_json::to_string_pretty(&out)?);
            }
        }
        _ => {
            println!("  {}", "could not determine bisect result — insufficient trace data.".yellow());
        }
    }

    println!();
    println!("  {}  {}  {}", "next steps:".dimmed(), "flux why <request-id>".cyan(), "root cause of a failing request".dimmed());
    println!("  {}  {}  {}", "           ".dimmed(), "flux trace <request-id>".cyan(), "full span trace".dimmed());
    println!();

    Ok(())
}

// ── Helpers ──────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct CommitData {
    sha:               String,
    first_seen:        String,
    total:             usize,
    failed:            usize,
    sample_request_id: Option<String>,
}

fn show_result(bad: &CommitData, good: &CommitData, json_output: bool) {
    println!("  {}  {}  ({}/{} failed)", "✗ BAD ".red().bold(),  short_sha(&bad.sha).red(),   bad.failed,  bad.total);
    println!("  {}  {}  ({}/{} failed)", "✔ GOOD".green(),       short_sha(&good.sha).green(), good.failed, good.total);
    if json_output {
        let _ = serde_json::to_string_pretty(&serde_json::json!({
            "first_bad": bad.sha,
            "last_good": good.sha,
        })).map(|s| println!("\n{s}"));
    }
}

fn short_sha(sha: &str) -> String {
    if sha.len() >= 7 { sha[..7].to_string() } else { sha.to_string() }
}

fn trunc(s: &str, n: usize) -> String {
    if s.len() > n { format!("{}…", &s[..n]) } else { s.to_string() }
}
