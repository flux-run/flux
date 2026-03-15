//! Opt-out CLI telemetry.
//!
//! Sends fire-and-forget events to PostHog to measure real-world CLI usage.
//! **No personal data is ever sent.** Every event contains only:
//!   - CLI version
//!   - OS name (linux / macos / windows)
//!   - CPU architecture (amd64 / arm64)
//!   - Event name + numeric/boolean properties
//!
//! ## Opt-out
//! Set any of these environment variables to disable telemetry entirely:
//!   FLUX_NO_TELEMETRY=1
//!   DO_NOT_TRACK=1
//!
//! See: https://fluxbase.co/docs/telemetry

use serde_json::{json, Value};

// Baked in at compile time via `FLUX_POSTHOG_KEY` env var.
// Falls back to "" if not set — telemetry is silently disabled in that case.
const POSTHOG_KEY: &str = match option_env!("FLUX_POSTHOG_KEY") {
    Some(k) => k,
    None => "",
};
const POSTHOG_HOST: &str = "https://analytics.fluxbase.co";

// Stable anonymous ID — no per-user or per-machine tracking.
// PostHog requires a distinct_id; we use a deterministic string so all CLI
// events land in one anonymous bucket.  Project-level segmentation comes from
// the `os` / `arch` / `version` properties, not from identity.
const DISTINCT_ID: &str = "flux-cli";

/// Returns `true` if the user has opted out of telemetry.
fn is_opted_out() -> bool {
    std::env::var("FLUX_NO_TELEMETRY").map(|v| !v.is_empty() && v != "0").unwrap_or(false)
        || std::env::var("DO_NOT_TRACK").map(|v| !v.is_empty() && v != "0").unwrap_or(false)
}

fn os_name() -> &'static str {
    if cfg!(target_os = "linux")   { "linux"   }
    else if cfg!(target_os = "macos")  { "macos"   }
    else if cfg!(target_os = "windows") { "windows" }
    else { "unknown" }
}

fn arch_name() -> &'static str {
    if cfg!(target_arch = "x86_64")  { "amd64" }
    else if cfg!(target_arch = "aarch64") { "arm64" }
    else { "unknown" }
}

/// Fire-and-forget telemetry capture.
///
/// Spawns a short-lived `tokio` task that POST-s to PostHog.
/// Returns immediately — callers never block on it.
/// All errors are silently swallowed.
pub fn capture(event: &str, extra: Value) {
    if is_opted_out() {
        return;
    }
    if POSTHOG_KEY.is_empty() {
        return;
    }

    let event = event.to_owned();

    let mut properties = json!({
        "$lib":     "flux-cli",
        "version":  env!("CARGO_PKG_VERSION"),
        "os":       os_name(),
        "arch":     arch_name(),
    });

    // Merge caller-supplied properties into the base properties object.
    if let (Some(base), Some(extra_map)) = (properties.as_object_mut(), extra.as_object()) {
        for (k, v) in extra_map {
            base.insert(k.clone(), v.clone());
        }
    }

    let payload = json!({
        "api_key":     POSTHOG_KEY,
        "event":       event,
        "distinct_id": DISTINCT_ID,
        "properties":  properties,
    });

    // Spawn a detached task; if the runtime is shutting down we just miss the event.
    tokio::spawn(async move {
        let _ = send_event(payload).await;
    });
}

async fn send_event(payload: Value) -> anyhow::Result<()> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()?;

    client
        .post(format!("{POSTHOG_HOST}/capture/"))
        .json(&payload)
        .send()
        .await?;

    Ok(())
}
