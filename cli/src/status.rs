use anyhow::Result;

use crate::config::resolve_auth;
use crate::grpc::list_logs;
use crate::process_state::{runtime_info, server_info};

pub async fn execute() -> Result<()> {
    let server = server_info();
    let runtime = runtime_info();

    let auth = resolve_auth(None, None);

    println!();

    let server_line = if server.running {
        let addr = server
            .port
            .map(|p| format!("localhost:{}", p))
            .unwrap_or_else(|| "localhost:?".to_string());
        format!("  server   \x1b[32m✓\x1b[0m  {}  running", addr)
    } else {
        "  server   \x1b[31m✗\x1b[0m  not running".to_string()
    };
    println!("{}", server_line);

    let runtime_line = if runtime.running {
        let addr = runtime
            .port
            .map(|p| format!("localhost:{}", p))
            .unwrap_or_else(|| "localhost:?".to_string());
        let entry = runtime.entry.unwrap_or_else(|| "unknown".to_string());
        format!("  runtime  \x1b[32m✓\x1b[0m  {}  serving {}", addr, entry)
    } else {
        "  runtime  \x1b[31m✗\x1b[0m  not running".to_string()
    };
    println!("{}", runtime_line);

    match auth {
        Ok(auth) => {
            let logs = list_logs(&auth.url, &auth.token, 100).await?;
            println!("  database \x1b[32m✓\x1b[0m  postgres  reachable");

            if let Some(last) = logs.first() {
                let time = short_time(&last.timestamp);
                let status_symbol = if last.status.eq_ignore_ascii_case("error") {
                    "\x1b[31m✗\x1b[0m"
                } else {
                    "\x1b[32m✓\x1b[0m"
                };
                println!();
                println!(
                    "  last execution  {}  {} {}  {}  {}ms",
                    time, last.method, last.path, status_symbol, last.duration_ms
                );
            }
        }
        Err(_) => {
            println!("  database \x1b[33m?\x1b[0m  configure with `flux init`");
        }
    }

    println!();

    Ok(())
}

fn short_time(ts: &str) -> String {
    if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(ts) {
        parsed
            .with_timezone(&chrono::Utc)
            .format("%H:%M:%S")
            .to_string()
    } else {
        ts.to_string()
    }
}
