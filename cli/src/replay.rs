use anyhow::{Result, bail};
use clap::Args;
use std::collections::{BTreeMap, BTreeSet};

const REPLAY_DIVERGENCE_EXIT_CODE: i32 = 2;
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_DIM: &str = "\x1b[90m";
const ANSI_RESET: &str = "\x1b[0m";

use crate::config::resolve_auth;
use crate::grpc::{get_trace, replay};

#[derive(Debug, Args)]
pub struct ReplayArgs {
    #[arg(value_name = "EXECUTION_ID")]
    pub execution_id: String,
    #[arg(long)]
    pub commit: bool,
    #[arg(long)]
    pub validate: bool,
    #[arg(long)]
    pub explain: bool,
    #[arg(long, value_name = "PATHS", value_delimiter = ',')]
    pub ignore: Vec<String>,
    #[arg(long, value_name = "INDEX")]
    pub from_index: Option<i32>,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
    #[arg(long)]
    pub diff: bool,
}

pub async fn execute(args: ReplayArgs) -> Result<()> {
    if args.validate && !args.commit {
        bail!(
            "--validate requires --commit so replay can compare live checkpoint results against recorded checkpoints"
        );
    }

    let auth = resolve_auth(args.url, args.token)?;
    let from_index = args.from_index.unwrap_or(0).max(0);
    let original = if args.diff {
        Some(get_trace(&auth.url, &auth.token, &args.execution_id).await?)
    } else {
        None
    };

    let short_id = if args.execution_id.len() >= 8 {
        &args.execution_id[..8]
    } else {
        &args.execution_id
    };

    println!();
    println!("  replaying {}…", short_id);
    println!();

    let response = replay(
        &auth.url,
        &auth.token,
        &args.execution_id,
        args.commit,
        from_index,
        args.validate,
    )
    .await?;

    let original = if original.is_some() || !response.steps.iter().any(|step| step.used_recorded) {
        original
    } else {
        Some(get_trace(&auth.url, &auth.token, &args.execution_id).await?)
    };
    let recorded_cache_hits = original
        .as_ref()
        .map(recorded_cache_hit_map)
        .unwrap_or_default();

    let status_symbol = if response.status == "ok" {
        "\x1b[32m✓\x1b[0m"
    } else {
        "\x1b[31m✗\x1b[0m"
    };

    if args.explain {
        print_explain_view(
            short_id,
            &response,
            &recorded_cache_hits,
            args.validate,
            &args.ignore,
        );
        if args.validate && response.divergence.is_some() {
            std::process::exit(REPLAY_DIVERGENCE_EXIT_CODE);
        }
        return Ok(());
    }

    println!(
        "  {}  {}  {}ms",
        status_symbol, response.status, response.duration_ms
    );

    if args.validate {
        println!("  validation  live checkpoints must match recorded checkpoints");
    }

    if args.diff {
        if let Some(original) = &original {
            println!();
            println!("  comparing original vs replay");
            println!();
            let original_status = colorize_status_label(&original.status, original.duration_ms);
            let replay_status = colorize_status_label(&response.status, response.duration_ms);
            println!(
                "  original  {:<18}  {}ms",
                original_status, original.duration_ms
            );
            println!(
                "  replay    {:<18}  {}ms",
                replay_status, response.duration_ms
            );
        }
    }

    if !response.error.is_empty() {
        println!("  error  {}", sanitize_replay_error(&response.error));
    }

    if let Some(divergence) = &response.divergence {
        let visible_diffs = filtered_diffs(&divergence.diffs, &args.ignore);
        println!("  divergence");
        println!(
            "    checkpoint  [{}] {}  {}",
            divergence.checkpoint_index,
            divergence.boundary.to_uppercase(),
            divergence.url,
        );
        println!("    expected    {}", divergence.expected_json);
        println!("    actual      {}", divergence.actual_json);
        if !visible_diffs.is_empty() {
            println!("    diff");
            for diff in visible_diffs {
                println!("      {} ({})", diff.path, diff.kind);
                println!("        expected  {}", diff.expected_json);
                println!("        actual    {}", diff.actual_json);
            }
        } else if !divergence.diffs.is_empty() && !args.ignore.is_empty() {
            println!("    diff        all matching changes hidden by --ignore");
        }
    }

    if !response.output.is_empty() && response.output != "null" {
        println!("  output  {}", format_replay_output(&response.output));
        // Machine-readable line for tooling (e.g. integration test runner).
        // The human-readable `output` line above may be formatted (decoded, summarised),
        // so we emit the raw JSON separately so parsers can rely on it.
        if args.diff {
            println!("  output_raw  {}", response.output);
        }
    }

    println!();
    for step in &response.steps {
        let source = replay_step_source_label(
            step,
            recorded_cache_hits
                .get(&step.call_index)
                .copied()
                .unwrap_or(false),
        );
        println!(
            "  [{}] {}  {}  {}ms  ({})",
            step.call_index,
            step.boundary.to_uppercase(),
            step.url,
            step.duration_ms,
            source
        );

        if args.diff {
            if step.used_recorded {
                println!("      response unchanged (recorded)");
            } else {
                println!("      response from live call");
            }
        }
    }

    if args.diff {
        if let Some(original) = &original {
            let original_output = first_non_empty_value(&[
                Some(original.response_json.clone()),
                if original.error.is_empty() {
                    None
                } else {
                    Some(original.error.clone())
                },
            ]);
            let replay_output = first_non_empty_value(&[
                if response.output.is_empty() || response.output == "null" {
                    None
                } else {
                    Some(response.output.clone())
                },
                if response.error.is_empty() {
                    None
                } else {
                    Some(response.error.clone())
                },
            ]);

            println!();
            println!("  output");
            for line in json_diff_lines(&original_output, &replay_output) {
                println!("  {}", colorize_diff_line(&line));
            }
        }
    }

    if !args.commit {
        println!();
        println!("  db writes suppressed — pass --commit to apply");
    }

    println!();

    if args.validate && response.divergence.is_some() {
        std::process::exit(REPLAY_DIVERGENCE_EXIT_CODE);
    }

    Ok(())
}

fn recorded_cache_hit_map(trace: &crate::grpc::TraceView) -> BTreeMap<i32, bool> {
    trace
        .checkpoints
        .iter()
        .filter_map(|checkpoint| {
            if checkpoint.boundary != "http" {
                return None;
            }

            let response = serde_json::from_slice::<serde_json::Value>(&checkpoint.response)
                .unwrap_or_default();
            let hit = response
                .get("cache")
                .and_then(|cache| cache.get("hit"))
                .and_then(|value| value.as_bool())
                .unwrap_or(false);

            Some((checkpoint.call_index, hit))
        })
        .collect()
}

fn replay_step_source_label(
    step: &crate::grpc::ReplayStepView,
    originally_cache_hit: bool,
) -> String {
    let mut segments = vec![step.source.clone()];
    if step.used_recorded && originally_cache_hit {
        segments.push("originally cache hit".to_string());
    }
    if step.validated {
        segments.push("validated".to_string());
    }
    segments.join(", ")
}

fn first_non_empty_value(candidates: &[Option<String>]) -> serde_json::Value {
    for item in candidates {
        if let Some(value) = item {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    return parsed;
                }
                return serde_json::Value::String(trimmed.to_string());
            }
        }
    }
    serde_json::Value::Null
}

fn json_diff_lines(original: &serde_json::Value, replay: &serde_json::Value) -> Vec<String> {
    let mut lines = Vec::new();
    diff_value("$", Some(original), Some(replay), &mut lines);

    if lines.is_empty() {
        vec!["unchanged".to_string()]
    } else {
        lines
    }
}

fn diff_value(
    path: &str,
    original: Option<&serde_json::Value>,
    replay: Option<&serde_json::Value>,
    out: &mut Vec<String>,
) {
    match (original, replay) {
        (None, None) => {}
        (None, Some(right)) => {
            out.push(format!("+ {} = {}", path, compact_json(right)));
        }
        (Some(left), None) => {
            out.push(format!("- {} = {}", path, compact_json(left)));
        }
        (Some(left), Some(right)) => {
            if left == right {
                return;
            }

            match (left, right) {
                (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
                    let mut keys = BTreeSet::new();
                    keys.extend(a.keys().cloned());
                    keys.extend(b.keys().cloned());

                    for key in keys {
                        let child_path = format!("{}.{}", path, key);
                        diff_value(&child_path, a.get(&key), b.get(&key), out);
                    }
                }
                (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
                    let max = a.len().max(b.len());
                    for idx in 0..max {
                        let child_path = format!("{}[{}]", path, idx);
                        diff_value(&child_path, a.get(idx), b.get(idx), out);
                    }
                }
                _ => {
                    out.push(format!("- {} = {}", path, compact_json(left)));
                    out.push(format!("+ {} = {}", path, compact_json(right)));
                }
            }
        }
    }
}

fn compact_json(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_string())
}

/// Decode a `__FLUX_B64:<base64>` string to plain text.
fn decode_flux_b64(s: &str) -> String {
    if let Some(b64) = s.strip_prefix("__FLUX_B64:") {
        use base64::Engine;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            if let Ok(text) = String::from_utf8(bytes) {
                return text;
            }
        }
    }
    s.to_string()
}

/// Walk a JSON value and decode any `__FLUX_B64:` strings in-place.
fn decode_b64_in_value(val: &mut serde_json::Value) {
    match val {
        serde_json::Value::String(s) if s.starts_with("__FLUX_B64:") => {
            *s = decode_flux_b64(s);
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                decode_b64_in_value(v);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                decode_b64_in_value(v);
            }
        }
        _ => {}
    }
}

/// Format replay output: decode base64 bodies and show a clean summary.
/// If the output is a net_response with a text body and status, show:
///   `<status> "<body>"` (e.g. `500 "Internal Server Error"`)
/// Otherwise fall back to compact decoded JSON.
fn format_replay_output(raw: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return raw.to_string();
    };
    decode_b64_in_value(&mut value);

    // If it's a net_response wrapper, show a concise summary
    if let Some(net) = value.get("net_response") {
        let status = net.get("status").and_then(|v| v.as_u64()).unwrap_or(0);
        let body = net
            .get("body")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let status_color = if status >= 400 { "\x1b[31m" } else { "\x1b[32m" };
        if !body.is_empty() {
            return format!("{}{}\x1b[0m  \"{}\"", status_color, status, body);
        }
        return format!("{}{} (no body)\x1b[0m", status_color, status);
    }

    serde_json::to_string(&value).unwrap_or_else(|_| raw.to_string())
}

fn colorize_diff_line(line: &str) -> String {
    if line.starts_with("+ ") {
        return format!("\x1b[32m{}\x1b[0m", line);
    }
    if line.starts_with("- ") {
        return format!("\x1b[31m{}\x1b[0m", line);
    }
    format!("\x1b[90m{}\x1b[0m", line)
}

fn colorize_status_label(status: &str, duration_ms: i32) -> String {
    let lower = status.to_ascii_lowercase();
    if lower == "ok" || lower == "success" {
        if duration_ms > 500 {
            return "\x1b[33m⚠ slow\x1b[0m".to_string();
        }
        return "\x1b[32m✓ ok\x1b[0m".to_string();
    }
    if lower == "error" || lower == "failed" {
        return "\x1b[31m✗ error\x1b[0m".to_string();
    }
    if duration_ms > 500 {
        return format!("\x1b[33m⚠ {}\x1b[0m", lower);
    }
    lower
}

fn sanitize_replay_error(raw: &str) -> String {
    let mut lines = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.contains("ext:core/") || trimmed.contains("flux:invoke") {
            continue;
        }
        lines.push(trimmed.to_string());
    }

    if lines.is_empty() {
        raw.lines()
            .next()
            .map(|line| line.trim().to_string())
            .unwrap_or_else(|| raw.trim().to_string())
    } else {
        lines.join("\n         ")
    }
}

#[derive(Default)]
struct DiffTreeNode {
    children: BTreeMap<String, DiffTreeNode>,
    expected: Option<String>,
    actual: Option<String>,
    kind: Option<String>,
}

fn colorize(text: impl AsRef<str>, color: &str) -> String {
    format!("{}{}{}", color, text.as_ref(), ANSI_RESET)
}

fn path_matches_ignore(path: &str, ignore_patterns: &[String]) -> bool {
    if ignore_patterns.is_empty() {
        return false;
    }

    let segments = parse_diff_path(path);
    ignore_patterns.iter().any(|pattern| {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            return false;
        }

        if path == trimmed {
            return true;
        }

        segments.iter().any(|segment| segment == trimmed)
    })
}

fn filtered_diffs<'a>(
    diffs: &'a [crate::grpc::ReplayFieldDiffView],
    ignore_patterns: &[String],
) -> Vec<&'a crate::grpc::ReplayFieldDiffView> {
    diffs
        .iter()
        .filter(|diff| !path_matches_ignore(&diff.path, ignore_patterns))
        .collect()
}

fn parse_diff_path(path: &str) -> Vec<String> {
    let mut segments = Vec::new();
    let chars: Vec<char> = path.chars().collect();
    let mut index = 0;

    if chars.first() == Some(&'$') {
        index += 1;
    }

    while index < chars.len() {
        match chars[index] {
            '.' => {
                index += 1;
            }
            '[' => {
                let start = index;
                index += 1;
                while index < chars.len() && chars[index] != ']' {
                    index += 1;
                }
                if index < chars.len() {
                    index += 1;
                }
                segments.push(chars[start..index].iter().collect());
            }
            _ => {
                let start = index;
                while index < chars.len() && chars[index] != '.' && chars[index] != '[' {
                    index += 1;
                }
                segments.push(chars[start..index].iter().collect());
            }
        }
    }

    segments
}

fn insert_diff(
    node: &mut DiffTreeNode,
    segments: &[String],
    expected: &str,
    actual: &str,
    kind: &str,
) {
    if segments.is_empty() {
        node.expected = Some(expected.to_string());
        node.actual = Some(actual.to_string());
        node.kind = Some(kind.to_string());
        return;
    }

    let child = node.children.entry(segments[0].clone()).or_default();
    insert_diff(child, &segments[1..], expected, actual, kind);
}

fn build_diff_tree(diffs: &[crate::grpc::ReplayFieldDiffView]) -> DiffTreeNode {
    let mut root = DiffTreeNode::default();
    for diff in diffs {
        let segments = parse_diff_path(&diff.path);
        insert_diff(
            &mut root,
            &segments,
            &diff.expected_json,
            &diff.actual_json,
            &diff.kind,
        );
    }
    root
}

fn render_diff_tree(name: Option<&str>, node: &DiffTreeNode, indent: usize) {
    let prefix = " ".repeat(indent);

    if let Some(name) = name {
        if node.expected.is_some() || !node.children.is_empty() {
            if let Some(kind) = &node.kind {
                println!("{}{} ({}):", prefix, name, kind);
            } else {
                println!("{}{}:", prefix, name);
            }
        }
    }

    let child_indent = if name.is_some() { indent + 2 } else { indent };
    let child_prefix = " ".repeat(child_indent);

    if let (Some(expected), Some(actual)) = (&node.expected, &node.actual) {
        println!("{}expected  {}", child_prefix, colorize(expected, ANSI_DIM));
        println!("{}actual    {}", child_prefix, colorize(actual, ANSI_RED));
    }

    for (child_name, child_node) in &node.children {
        render_diff_tree(Some(child_name), child_node, child_indent);
    }
}

fn print_explain_view(
    short_id: &str,
    response: &crate::grpc::ReplayView,
    recorded_cache_hits: &BTreeMap<i32, bool>,
    validate: bool,
    ignore_patterns: &[String],
) {
    println!("  Execution Replay ({})", short_id);
    println!();

    let status_label = if response.status == "ok" {
        colorize("ok", ANSI_GREEN)
    } else {
        colorize("error", ANSI_RED)
    };

    println!("  status      {}", status_label);
    println!("  duration    {}ms", response.duration_ms);
    if validate {
        println!("  validation  {}", colorize("enabled", ANSI_YELLOW));
    }
    if !ignore_patterns.is_empty() {
        println!("  ignore      {}", ignore_patterns.join(", "));
    }

    println!();
    for step in &response.steps {
        let summary = colorize(
            replay_step_source_label(
                step,
                recorded_cache_hits
                    .get(&step.call_index)
                    .copied()
                    .unwrap_or(false),
            ),
            if step.source == "recorded" {
                ANSI_GREEN
            } else if step.source == "live" && step.validated {
                ANSI_YELLOW
            } else {
                ANSI_DIM
            },
        );

        let diverged = response
            .divergence
            .as_ref()
            .map(|divergence| divergence.checkpoint_index == step.call_index)
            .unwrap_or(false);

        if diverged {
            println!(
                "  [{}] {} {}  {}ms  {}  {}",
                step.call_index,
                step.boundary.to_lowercase(),
                step.url,
                step.duration_ms,
                summary,
                colorize("DIVERGED", ANSI_RED),
            );
        } else {
            println!(
                "  [{}] {} {}  {}ms  {}",
                step.call_index,
                step.boundary.to_lowercase(),
                step.url,
                step.duration_ms,
                summary,
            );
        }
    }

    if let Some(divergence) = &response.divergence {
        let visible_diffs = filtered_diffs(&divergence.diffs, ignore_patterns);
        println!();
        println!("  {}", colorize("First Divergence", ANSI_RED));
        println!(
            "  [{}] {} {}",
            divergence.checkpoint_index,
            divergence.boundary.to_lowercase(),
            divergence.url,
        );

        if divergence.diffs.is_empty() {
            println!(
                "    expected  {}",
                colorize(&divergence.expected_json, ANSI_DIM)
            );
            println!(
                "    actual    {}",
                colorize(&divergence.actual_json, ANSI_RED)
            );
        } else {
            if visible_diffs.is_empty() {
                println!(
                    "    {}",
                    colorize("all matching field diffs hidden by --ignore", ANSI_DIM)
                );
            } else {
                let owned_diffs: Vec<crate::grpc::ReplayFieldDiffView> =
                    visible_diffs.into_iter().cloned().collect();
                let tree = build_diff_tree(&owned_diffs);
                render_diff_tree(None, &tree, 4);
            }
        }
    }

    if !response.error.is_empty() {
        println!();
        println!(
            "  error  {}",
            colorize(sanitize_replay_error(&response.error), ANSI_RED)
        );
    }

    if !response.output.is_empty() && response.output != "null" {
        println!();
        println!("  output  {}", format_replay_output(&response.output));
    }

    println!();
}

#[cfg(test)]
mod tests {
    use super::{recorded_cache_hit_map, replay_step_source_label};

    #[test]
    fn marks_recorded_step_as_originally_cache_hit() {
        let step = crate::grpc::ReplayStepView {
            call_index: 2,
            boundary: "http".to_string(),
            url: "https://example.test/jwks".to_string(),
            used_recorded: true,
            duration_ms: 3,
            source: "recorded".to_string(),
            validated: false,
        };

        assert_eq!(
            replay_step_source_label(&step, true),
            "recorded, originally cache hit"
        );
    }

    #[test]
    fn preserves_validation_suffix_for_live_step() {
        let step = crate::grpc::ReplayStepView {
            call_index: 2,
            boundary: "http".to_string(),
            url: "https://example.test/jwks".to_string(),
            used_recorded: false,
            duration_ms: 3,
            source: "live".to_string(),
            validated: true,
        };

        assert_eq!(replay_step_source_label(&step, false), "live, validated");
    }

    #[test]
    fn extracts_cache_hits_from_original_trace() {
        let trace = crate::grpc::TraceView {
            execution_id: "exec-1".to_string(),
            method: "GET".to_string(),
            path: "/".to_string(),
            status: "200".to_string(),
            duration_ms: 5,
            error: String::new(),
            request_json: "{}".to_string(),
            response_json: "{}".to_string(),
            checkpoints: vec![
                crate::grpc::TraceCheckpoint {
                    call_index: 1,
                    boundary: "http".to_string(),
                    request: br#"{}"#.to_vec(),
                    response: br#"{"cache":{"hit":true,"source":"memory","age_ms":1000}}"#.to_vec(),
                    duration_ms: 1,
                },
                crate::grpc::TraceCheckpoint {
                    call_index: 2,
                    boundary: "http".to_string(),
                    request: br#"{}"#.to_vec(),
                    response: br#"{"status":200}"#.to_vec(),
                    duration_ms: 1,
                },
            ],
            logs: vec![],
        };

        let hits = recorded_cache_hit_map(&trace);
        assert_eq!(hits.get(&1), Some(&true));
        assert_eq!(hits.get(&2), Some(&false));
    }
}
