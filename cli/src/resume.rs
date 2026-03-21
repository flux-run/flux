use anyhow::Result;
use clap::Args;

use crate::config::resolve_auth;
use crate::grpc::resume;

#[derive(Debug, Args)]
pub struct ResumeArgs {
    #[arg(value_name = "EXECUTION_ID")]
    pub execution_id: String,
    #[arg(long, value_name = "INDEX")]
    pub from: Option<i32>,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
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

pub async fn execute(args: ResumeArgs) -> Result<()> {
    let auth = resolve_auth(args.url, args.token)?;

    let short_id = if args.execution_id.len() >= 8 {
        &args.execution_id[..8]
    } else {
        &args.execution_id
    };

    let response = resume(&auth.url, &auth.token, &args.execution_id, args.from).await?;

    println!();
    println!(
        "  resuming {}… from checkpoint {}",
        short_id, response.from_index
    );
    println!();

    for step in &response.steps {
        let source = if step.used_recorded {
            "recorded"
        } else {
            "live"
        };
        println!(
            "  [{}] {}  {}  {}ms  ({})",
            step.call_index,
            step.boundary.to_uppercase(),
            step.url,
            step.duration_ms,
            source
        );
    }

    println!();
    let status_symbol = if response.status == "ok" {
        "✓"
    } else {
        "✗"
    };
    println!(
        "  {}  {}  {}ms",
        status_symbol, response.status, response.duration_ms
    );

    if !response.error.is_empty() {
        println!("  error  {}", response.error);
    }

    if !response.output.is_empty() && response.output != "null" {
        // Decode __FLUX_B64 bodies so the output is human-readable and
        // parseable by tooling (e.g. the integration test runner).
        let decoded_output = if let Ok(mut val) =
            serde_json::from_str::<serde_json::Value>(&response.output)
        {
            decode_b64_in_value(&mut val);
            serde_json::to_string(&val).unwrap_or_else(|_| response.output.clone())
        } else {
            response.output.clone()
        };
        println!("  output {}", decoded_output);
    }

    println!();
    Ok(())
}
