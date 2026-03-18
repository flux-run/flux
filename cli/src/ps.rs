use anyhow::Result;

use crate::process_state::{format_uptime, runtime_info, server_info};

pub async fn execute() -> Result<()> {
    let server = server_info();
    let runtime = runtime_info();

    println!();
    print_row(
        &server.name,
        server.pid,
        server.port,
        server.entry.as_deref(),
        server.running,
        server.started_at,
    );
    print_row(
        &runtime.name,
        runtime.pid,
        runtime.port,
        runtime.entry.as_deref(),
        runtime.running,
        runtime.started_at,
    );
    println!();

    Ok(())
}

fn print_row(
    name: &str,
    pid: Option<i32>,
    port: Option<u16>,
    entry: Option<&str>,
    running: bool,
    started_at: Option<std::time::SystemTime>,
) {
    if !running {
        println!("  {:<12}  not running", name);
        return;
    }

    let pid = pid
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());
    let port = port
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());
    let uptime = format_uptime(started_at);

    if let Some(entry) = entry {
        println!(
            "  {:<12}  pid {:<7}  port {:<6}  uptime {:<4}  serving {}",
            name, pid, port, uptime, entry,
        );
    } else {
        println!(
            "  {:<12}  pid {:<7}  port {:<6}  uptime {}",
            name, pid, port, uptime,
        );
    }
}
