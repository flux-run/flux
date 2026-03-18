use std::path::PathBuf;
use std::time::SystemTime;

use anyhow::{Context, Result};

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub name: &'static str,
    pub pid: Option<i32>,
    pub port: Option<u16>,
    pub entry: Option<String>,
    pub running: bool,
    pub started_at: Option<SystemTime>,
}

pub fn server_info() -> ProcessInfo {
    let pid_path = flux_dir().join("server.pid");
    let pid = read_pid(&pid_path).ok().flatten();
    let port = read_port(flux_dir().join("server.port")).ok().flatten();
    let running = pid.map(is_pid_running).unwrap_or(false);
    let started_at = std::fs::metadata(&pid_path)
        .ok()
        .and_then(|m| m.modified().ok());

    ProcessInfo {
        name: "flux-server",
        pid,
        port,
        entry: None,
        running,
        started_at,
    }
}

pub fn runtime_info() -> ProcessInfo {
    let pid_path = flux_dir().join("runtime.pid");
    let pid = read_pid(&pid_path).ok().flatten();
    let port = read_port(flux_dir().join("runtime.port")).ok().flatten();
    let entry = read_entry(flux_dir().join("runtime.entry")).ok().flatten();
    let running = pid.map(is_pid_running).unwrap_or(false);
    let started_at = std::fs::metadata(&pid_path)
        .ok()
        .and_then(|m| m.modified().ok());

    ProcessInfo {
        name: "flux-runtime",
        pid,
        port,
        entry,
        running,
        started_at,
    }
}

pub fn format_uptime(started_at: Option<SystemTime>) -> String {
    let Some(started_at) = started_at else {
        return "-".to_string();
    };
    let Ok(elapsed) = started_at.elapsed() else {
        return "-".to_string();
    };

    let seconds = elapsed.as_secs();
    if seconds >= 3600 {
        format!("{}h", seconds / 3600)
    } else if seconds >= 60 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}s", seconds)
    }
}

fn read_pid(path: &PathBuf) -> Result<Option<i32>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let pid = raw
        .trim()
        .parse::<i32>()
        .with_context(|| format!("invalid pid in {}", path.display()))?;
    Ok(Some(pid))
}

fn read_port(path: PathBuf) -> Result<Option<u16>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let port = raw
        .trim()
        .parse::<u16>()
        .with_context(|| format!("invalid port in {}", path.display()))?;
    Ok(Some(port))
}

fn read_entry(path: PathBuf) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let entry = raw.trim().to_string();
    if entry.is_empty() {
        return Ok(None);
    }
    Ok(Some(entry))
}

fn flux_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flux")
}

#[cfg(unix)]
fn is_pid_running(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    std::process::Command::new("kill")
        .arg("-0")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_pid_running(pid: i32) -> bool {
    pid > 0
}
