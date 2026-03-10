use crate::client::ApiClient;
use colored::Colorize;
use serde_json::Value;

fn format_timestamp(ts: &str) -> String {
    ts.get(..19)
        .map(|s| s.replace('T', " "))
        .unwrap_or_else(|| ts.to_string())
}

fn colorize_level(level: &str) -> colored::ColoredString {
    match level.to_uppercase().as_str() {
        "ERROR" | "ERR"     => level.to_uppercase().red().bold(),
        "WARN"  | "WARNING" => level.to_uppercase().yellow().bold(),
        "DEBUG"             => level.to_uppercase().dimmed(),
        _                   => level.to_uppercase().normal(),
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
        _          => source.green(),  // function (default)
    }
}

pub async fn execute(request_id: String) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    // Percent-encode `:` in the request_id in case it contains them
    let encoded_id = request_id.replace(':', "%3A");
    let url = format!("{}/traces/{}", client.base_url, encoded_id);

    let res: reqwest::Response = client.client.get(&url).send().await?;

    if res.status() == reqwest::StatusCode::NOT_FOUND {
        eprintln!("{} no trace found for request ID: {}", "✗".red(), request_id.bold());
        return Ok(());
    }
    if !res.status().is_success() {
        anyhow::bail!("API error: {}", res.status());
    }

    let body: Value = res.json().await?;
    let data = &body["data"];
    let spans = data["spans"].as_array().cloned().unwrap_or_default();
    let span_count = data["span_count"].as_u64().unwrap_or(spans.len() as u64);

    if spans.is_empty() {
        println!("{} No spans found for request ID: {}", "ℹ".blue(), request_id.cyan().bold());
        println!("  Make sure the request was routed through the gateway and logs are enabled.");
        return Ok(());
    }

    println!();
    println!("{} {}", "Trace".bold().white(), request_id.cyan().bold());
    println!("{} spans\n", span_count.to_string().dimmed());

    for span in &spans {
        let ts       = span["timestamp"].as_str().unwrap_or("?");
        let source   = span["source"].as_str().unwrap_or("?");
        let resource = span["resource"].as_str().unwrap_or("?");
        let level    = span["level"].as_str().unwrap_or("info");
        let message  = span["message"].as_str().unwrap_or("");

        println!(
            "  {}  [{}{}{}]  {}  {}",
            format_timestamp(ts).dimmed(),
            colorize_source(source),
            "/".dimmed(),
            resource.bold(),
            colorize_level(level),
            message,
        );
    }

    // Summary line
    println!("\n  {} span(s) in trace", span_count.to_string().bold());

    println!();
    Ok(())
}
