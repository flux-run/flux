//! `flux doctor` — environment health check **and** incident diagnosis.
//!
//! Without arguments: inspects every layer of the developer environment and
//! prints a concise status report.
//!
//! With a request-id: loads trace spans, state mutations, and heuristics to
//! produce an automatic incident diagnosis:
//!
//! ```text
//! flux doctor 550e8400
//!
//! REQUEST
//! ────────────────
//! POST /signup
//! function: create_user
//! duration: 3210ms
//! status:   500
//!
//! ROOT CAUSE
//! ────────────────
//! ⚡ stripe.charge timed out after 10000ms
//!
//! EVIDENCE
//! ────────────────
//! stripe.charge   3200ms  ⚠ slow
//! db.users        12ms
//!
//! SUGGESTED ACTIONS
//! ────────────────
//! • Increase Stripe timeout above 3s
//! • Add retry for stripe.charge
//! ```
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
use serde_json::Value;

use crate::client::ApiClient;
use crate::config::{Config, ProjectConfig};
use crate::sdk::parse_local_version;
use crate::why::{diff_json, json_scalar};

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

pub async fn execute(request_id: Option<String>) -> anyhow::Result<()> {
    if let Some(rid) = request_id {
        return execute_diagnosis(rid).await;
    }
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

// ─── Incident diagnosis ───────────────────────────────────────────────────────

fn trunc_d(s: &str, n: usize) -> String {
    if s.len() > n { format!("{}…", &s[..n]) } else { s.to_string() }
}

async fn execute_diagnosis(request_id: String) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    // ── Fetch trace + mutations + previous request ────────────────────────
    let trace_url = format!("{}/traces/{}?slow_ms=0", client.base_url, request_id);
    let trace_body: Value = match client.client.get(&trace_url).send().await {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
        _ => Value::Null,
    };

    let mut_url = format!("{}/db/mutations?request_id={}&limit=20", client.base_url, request_id);
    let mut_body: Value = match client.client.get(&mut_url).send().await {
        Ok(r) if r.status().is_success() => r.json().await.unwrap_or_default(),
        _ => Value::Null,
    };

    // ── Unpack trace fields ───────────────────────────────────────────────
    let empty_vec: Vec<Value> = vec![];
    let spans: &Vec<Value> = trace_body.get("spans")
        .and_then(|s| s.as_array()).unwrap_or(&empty_vec);

    let request  = trace_body.get("request").unwrap_or(&Value::Null);
    let method   = request["method"].as_str().unwrap_or("?");
    let path     = request["path"].as_str().unwrap_or("?");
    let function = request["function"].as_str().unwrap_or("?");
    let status   = request["status"].as_i64().unwrap_or(0);
    let elapsed  = trace_body["total_ms"].as_i64().unwrap_or(0);
    let is_error = status >= 400 || spans.iter().any(|s| s["span_type"] == "error");

    let first_span_ts = spans.first()
        .and_then(|s| s["timestamp"].as_str()).unwrap_or("").to_string();

    // previous request
    let prev_req: Option<Value> = if !first_span_ts.is_empty() {
        let prev_url = format!(
            "{}/traces?before={}&limit=1&exclude={}",
            client.base_url,
            urlencoding::encode(&first_span_ts),
            urlencoding::encode(&request_id),
        );
        if let Ok(res) = client.client.get(&prev_url).send().await {
            if res.status().is_success() {
                let body: Value = res.json().await.unwrap_or_default();
                body["traces"].as_array().and_then(|a| a.first()).cloned()
            } else { None }
        } else { None }
    } else { None };

    // ── Mutations ─────────────────────────────────────────────────────────
    let mutations: Vec<Value> = mut_body.get("mutations")
        .and_then(|m| m.as_array()).cloned().unwrap_or_default();

    // ── Error span ────────────────────────────────────────────────────────
    let error_span: Option<&Value> = spans.iter()
        .find(|s| s["span_type"] == "error");
    let error_msg = error_span
        .and_then(|s| s["message"].as_str())
        .unwrap_or("");

    // ── Heuristics ────────────────────────────────────────────────────────
    struct Finding {
        emoji:       &'static str,
        root_cause:  String,
        likely:      String,
        actions:     Vec<String>,
    }
    let mut findings: Vec<Finding> = Vec::new();

    // Span analysis: collect slow / notable spans
    let work_types = ["tool", "http_request", "function", "db", "db_query",
                      "workflow_step", "agent_step", "gateway_request"];
    let mut work_spans: Vec<&Value> = spans.iter()
        .filter(|s| work_types.contains(&s["span_type"].as_str().unwrap_or("")))
        .collect();
    work_spans.sort_by_key(|s| {
        let e = s["elapsed_ms"].as_i64().unwrap_or(0);
        let d = s["delta_ms"].as_i64().unwrap_or(0);
        e - d
    });

    // H1 — slow external tool / HTTP call
    for s in &work_spans {
        let st = s["span_type"].as_str().unwrap_or("");
        if st != "tool" && st != "http_request" { continue; }
        let ms = s["delta_ms"].as_i64().unwrap_or(0)
            .max(s["elapsed_ms"].as_i64().unwrap_or(0));
        if ms < 1_000 { continue; }
        let name = s["resource"].as_str().unwrap_or(s["source"].as_str().unwrap_or("external call"));
        let is_timeout = error_msg.to_lowercase().contains("timeout");
        let cause = if is_timeout {
            format!("{} timed out after {}ms", trunc_d(name, 40), ms)
        } else {
            format!("{} took {}ms (slow)", trunc_d(name, 40), ms)
        };
        let mut acts = vec![
            format!("Increase timeout above {}ms", (ms / 1000 + 1) * 1000),
            format!("Add retry with exponential backoff for {}", trunc_d(name, 30)),
        ];
        if is_timeout { acts.push("Check network latency to the external service".to_string()); }
        findings.push(Finding {
            emoji: "⚡",
            root_cause: cause,
            likely: "External tool latency exceeded threshold.".to_string(),
            actions: acts,
        });
        break; // one finding per category
    }

    // H2 — slow DB query
    for s in &work_spans {
        let st = s["span_type"].as_str().unwrap_or("");
        if st != "db" && st != "db_query" { continue; }
        let ms = s["delta_ms"].as_i64().unwrap_or(0)
            .max(s["elapsed_ms"].as_i64().unwrap_or(0));
        if ms < 1_000 { continue; }
        let table = s["resource"].as_str().unwrap_or("unknown table");
        findings.push(Finding {
            emoji: "🗄",
            root_cause: format!("Database query on {} took {}ms", trunc_d(table, 30), ms),
            likely: "Missing index or expensive scan.".to_string(),
            actions: vec![
                format!("Inspect query plan: flux explain"),
                format!("Check indexes on {}", trunc_d(table, 30)),
                "Add read replica for heavy read queries".to_string(),
            ],
        });
        break;
    }

    // H3 — null / undefined access
    if error_msg.contains("undefined") || error_msg.to_lowercase().contains("null") ||
       error_msg.contains("Cannot read") {
        findings.push(Finding {
            emoji: "⚠",
            root_cause: trunc_d(error_msg, 80).to_string(),
            likely: "Null or undefined access in function code.".to_string(),
            actions: vec![
                format!("Add null guard in {}", function),
                "Validate incoming payload fields before use".to_string(),
                format!("Run: flux trace debug {} to step through", &request_id[..request_id.len().min(12)]),
            ],
        });
    }

    // H4 — race condition / same-row previous request
    let mutated_row_keys: Vec<String> = mutations.iter().filter_map(|m| {
        let table = m["table_name"].as_str()?;
        let id = m["after_state"].get("id").or_else(|| m["before_state"].get("id"))?;
        if let Some(s) = id.as_str() { Some(format!("{}.id={}", table, trunc_d(s, 8))) }
        else if let Some(n) = id.as_i64() { Some(format!("{}.id={}", table, n)) }
        else { None }
    }).collect();

    let prev_also_mutated = mutated_row_keys.iter().any(|_key| {
        prev_req.as_ref()
            .and_then(|p| p["request_id"].as_str())
            .map(|_| !mutated_row_keys.is_empty())
            .unwrap_or(false)
    });

    if prev_also_mutated && !mutated_row_keys.is_empty() {
        let row = &mutated_row_keys[0];
        findings.push(Finding {
            emoji: "⚠",
            root_cause: format!("Previous request also modified {}", row),
            likely: "Possible race condition or state dependency between requests.".to_string(),
            actions: vec![
                "Add optimistic locking (version check) on update".to_string(),
                "Ensure idempotent writes for retry scenarios".to_string(),
                format!("Check: flux state history {}", row.replace(".id=", " ")),
            ],
        });
    }

    // H5 — generic 5xx with no other finding  
    if findings.is_empty() && is_error {
        let cause = if !error_msg.is_empty() {
            trunc_d(error_msg, 80).to_string()
        } else {
            format!("HTTP {} from {}", status, function)
        };
        findings.push(Finding {
            emoji: "✗",
            root_cause: cause,
            likely: if status == 500 {
                "Unhandled exception in function.".to_string()
            } else {
                format!("Request returned HTTP {}.", status)
            },
            actions: vec![
                format!("Deep-dive: flux trace debug {}", &request_id[..request_id.len().min(12)]),
                format!("View full log context: flux why {}", &request_id[..request_id.len().min(12)]),
            ],
        });
    }

    // ─── Print ────────────────────────────────────────────────────────────
    println!();

    // REQUEST block
    let sep = "─".repeat(32);
    println!("{}", "REQUEST".bold());
    println!("{}", sep.dimmed());
    println!("  {} {}", method.bold(), path.bold());
    println!("  function: {}", function.cyan());
    if elapsed > 0 {
        let dur_s = if elapsed >= 1_000 {
            format!("{:.2}s", elapsed as f64 / 1000.0)
        } else {
            format!("{}ms", elapsed)
        };
        println!("  duration: {}", if elapsed > 1000 { dur_s.yellow().to_string() } else { dur_s });
    }
    let status_str = status.to_string();
    println!("  status:   {}",
        if status >= 500 { status_str.red().bold().to_string() }
        else if status >= 400 { status_str.yellow().bold().to_string() }
        else { status_str.green().to_string() }
    );

    // ROOT CAUSE + LIKELY ISSUE
    if let Some(f) = findings.first() {
        println!();
        println!("{}", "ROOT CAUSE".bold());
        println!("{}", sep.dimmed());
        println!("  {} {}", f.emoji, f.root_cause.red());
        println!();
        println!("{}", "LIKELY ISSUE".bold());
        println!("{}", sep.dimmed());
        println!("  {}", f.likely);
    } else if !error_msg.is_empty() {
        println!();
        println!("{}", "ROOT CAUSE".bold());
        println!("{}", sep.dimmed());
        println!("  {}", error_msg.red());
    }

    // EVIDENCE
    if !work_spans.is_empty() {
        println!();
        println!("{}", "EVIDENCE".bold());
        println!("{}", sep.dimmed());
        for s in work_spans.iter().take(8) {
            let name = s["resource"].as_str()
                .or_else(|| s["source"].as_str()).unwrap_or("?");
            let ms = s["delta_ms"].as_i64().unwrap_or(0)
                .max(s["elapsed_ms"].as_i64().unwrap_or(0));
            let is_slow = s["is_slow"].as_bool().unwrap_or(false) || ms > 500;
            let ms_str = if ms >= 1_000 { format!("{:.1}s", ms as f64 / 1000.0) } else { format!("{}ms", ms) };
            let slow = if is_slow { "  ⚠ slow".yellow().to_string() } else { String::new() };
            println!(
                "  {}  {}{}",
                format!("{:<36}", trunc_d(name, 36)),
                if is_slow { ms_str.yellow().to_string() } else { ms_str.dimmed().to_string() },
                slow,
            );
        }
    }

    // DATA CHANGES
    if !mutations.is_empty() {
        println!();
        println!("{}", "DATA CHANGES".bold());
        println!("{}", sep.dimmed());
        for m in mutations.iter().take(5) {
            let table  = m["table_name"].as_str().unwrap_or("?");
            let op     = m["operation"].as_str().unwrap_or("?");
            let row_id = m["after_state"].get("id")
                .or_else(|| m["before_state"].get("id"))
                .and_then(|v| v.as_str().map(|s| trunc_d(s, 8))
                    .or_else(|| v.as_i64().map(|n| n.to_string())))
                .unwrap_or_default();

            println!(
                "  {}.id={}  {}",
                table.cyan(),
                row_id.dimmed(),
                match op { "insert" => op.green().to_string(), "delete" => op.red().to_string(), _ => op.yellow().to_string() },
            );

            if op == "update" {
                let diffs = diff_json(&m["before_state"], &m["after_state"]);
                for (key, old_v, new_v) in diffs.iter().take(4) {
                    println!("    {}  {} → {}", format!("{key}:").dimmed(), old_v.red(), new_v.green());
                }
            } else if op == "insert" {
                if let Some(obj) = m["after_state"].as_object() {
                    let skip = ["id","created_at","updated_at","tenant_id","project_id"];
                    for (k, v) in obj.iter().filter(|(k,_)| !skip.contains(&k.as_str())).take(3) {
                        println!("    {}  {}", format!("{k}:").dimmed(), json_scalar(v).green());
                    }
                }
            }
        }
    }

    // RELATED REQUEST
    if let Some(prev) = &prev_req {
        let p_method = prev["method"].as_str().unwrap_or("?");
        let p_path   = prev["path"].as_str().unwrap_or("?");
        let p_ms     = prev["duration_ms"].as_i64().unwrap_or(0);
        let p_status = prev["status"].as_i64().unwrap_or(0);

        let gap = prev["started_at"].as_str().and_then(|prev_ts| {
            let t0 = chrono::DateTime::parse_from_rfc3339(&first_span_ts).ok()?;
            let t1 = chrono::DateTime::parse_from_rfc3339(prev_ts).ok()?;
            let ms = (t0 - t1).num_milliseconds();
            if ms <= 0 { return None; }
            Some(if ms < 1_000 { format!("{}ms before", ms) } else { format!("{:.1}s before", ms as f64/1000.0) })
        });

        println!();
        println!("{}", "RELATED REQUEST".bold());
        println!("{}", sep.dimmed());
        let p_icon = match p_status { 200..=299 => "✔".green().bold().to_string(), 400..=499 => "!".yellow().bold().to_string(), _ => "✗".red().bold().to_string() };
        let gap_s = gap.map(|g| format!("  ({})", g)).unwrap_or_default();
        println!("  {} {} {}  {}ms{}", p_icon, p_method.bold(), trunc_d(p_path, 40), p_ms, gap_s.dimmed());
        for row in &mutated_row_keys {
            println!("  {}  {}", "⚠ modified".yellow(), row.yellow());
        }
    }

    // SUGGESTED ACTIONS
    println!();
    println!("{}", "SUGGESTED ACTIONS".bold());
    println!("{}", sep.dimmed());
    if findings.is_empty() {
        println!("  • Run: flux why {}", &request_id[..request_id.len().min(12)]);
        println!("  • Run: flux trace debug {}", &request_id[..request_id.len().min(12)]);
    } else {
        for f in &findings {
            for action in &f.actions {
                println!("  • {}", action);
            }
        }
        // Always append deep-dive links
        println!("  • {}", format!("flux why {}", &request_id[..request_id.len().min(12)]).cyan());
        println!("  • {}", format!("flux trace debug {}", &request_id[..request_id.len().min(12)]).cyan());
    }

    println!();
    Ok(())
}
