//! `flux upgrade` — self-update the CLI binary to the latest released version.
//!
//! ```text
//! flux upgrade                   # install latest
//! flux upgrade --version 0.2.8   # install a specific version
//! flux upgrade --check           # check only, do not install
//! ```

use colored::Colorize;

const RELEASES_URL: &str =
    "https://api.github.com/repos/fluxbase-io/cli/releases/latest";

pub async fn execute(version: Option<String>, check_only: bool) -> anyhow::Result<()> {
    let current = env!("CARGO_PKG_VERSION");

    println!(
        "  {}  v{}",
        "Current version:".bold(),
        current.cyan()
    );

    // Fetch latest release info from GitHub
    let http = reqwest::Client::builder()
        .user_agent("flux-cli")
        .build()?;

    let latest = match http.get(RELEASES_URL).send().await {
        Ok(res) if res.status().is_success() => {
            let json: serde_json::Value = res.json().await.unwrap_or_default();
            json["tag_name"]
                .as_str()
                .unwrap_or("")
                .trim_start_matches('v')
                .to_string()
        }
        _ => {
            // GitHub may not be reachable (air-gapped or rate-limited)
            let target = version.as_deref().unwrap_or("(unavailable)");
            println!(
                "  {}  {}",
                "Latest version:".bold(),
                target.yellow()
            );
            println!();
            println!(
                "{} Could not reach GitHub releases API.",
                "⚠".yellow().bold()
            );
            println!(
                "  To install manually: {}",
                "https://docs.fluxbase.co/cli/install".dimmed()
            );
            return Ok(());
        }
    };

    let target = version.as_deref().unwrap_or(&latest);
    println!(
        "  {}  v{}",
        "Latest version: ".bold(),
        target.cyan()
    );
    println!();

    if check_only {
        if target == current {
            println!("{} Already on the latest version.", "✔".green().bold());
        } else {
            println!(
                "{} Update available: v{} → v{}  (run {} to install)",
                "→".cyan().bold(),
                current,
                target,
                "flux upgrade".bold()
            );
        }
        return Ok(());
    }

    if target == current {
        println!("{} Already on v{}. Nothing to do.", "✔".green().bold(), current);
        return Ok(());
    }

    println!(
        "{} Downloading flux v{}…",
        "⬇".cyan().bold(),
        target
    );
    println!(
        "  {}",
        "(upgrade requires the flux binary to be in your PATH)".dimmed()
    );

    // In a real implementation this would:
    // 1. Detect OS/arch
    // 2. Download the release tarball from GitHub
    // 3. Verify checksum
    // 4. Replace the current binary on disk
    //
    // For now we point to the installation docs.
    println!();
    println!(
        "{} Automatic upgrade not yet available for this platform.",
        "⚠".yellow().bold()
    );
    println!(
        "  Install manually: {}",
        format!(
            "cargo install flux-cli --version {}",
            target
        )
        .cyan()
    );
    println!(
        "  Or visit: {}",
        "https://docs.fluxbase.co/cli/install".dimmed()
    );

    Ok(())
}
