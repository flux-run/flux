//! `flux toolchain` — manage pinned language runtimes and compilers.
//!
//! Toolchains are downloaded once to `~/.flux/toolchains/<lang>/<version>/`
//! and reused by both the CLI (`flux function build`) and the server runtime.
//!
//! ## Version pins (single source of truth)
//! All versions come from `crate::new_function::VERSIONS`.  The server reads the
//! same manifest, so CLI and server always use identical toolchain versions.
//!
//! ## Commands
//!   flux toolchain list               — show all languages + versions + install status
//!   flux toolchain install <lang>     — download toolchain for one language
//!   flux toolchain install all        — download all 14 toolchains
//!   flux toolchain which <lang>       — print path to binary for <lang>
//!
//! ## Storage layout
//! ```
//! ~/.flux/toolchains/
//!   deno/2.3.0/deno                   ← TypeScript (Deno)
//!   node/22.0.0/node                  ← JavaScript (Node.js)
//!   rust/1.87.0/cargo                 ← Rust (via rustup target)
//!   go/1.24.0/go                      ← Go
//!   python/3.12.0/python3             ← Python
//!   zig/0.13.0/zig                    ← Zig
//!   wasi-sdk/24.0/clang               ← C / C++
//!   assemblyscript/0.27.0/asc         ← AssemblyScript
//!   dotnet/9.0/dotnet                 ← C# (.NET)
//!   swift/6.0.0/swift                 ← Swift
//!   kotlin/2.1.0/kotlinc              ← Kotlin
//!   java/21.0/java                    ← Java (GraalVM Native)
//!   ruby/3.3.0/ruby                   ← Ruby
//! ```
//!
//! ## Download sources
//! Each toolchain has a known download URL template per OS/arch.
//! `flux toolchain install` resolves OS+arch, fetches the archive, extracts it,
//! and symlinks the binary to `~/.flux/toolchains/<lang>/<version>/<binary>`.

use colored::Colorize;

use crate::new_function::VERSIONS;

// ── Toolchain descriptors ─────────────────────────────────────────────────────

/// One installed (or installable) toolchain.
pub struct Toolchain {
    pub lang:    &'static str,
    pub version: &'static str,
    pub binary:  &'static str,   // binary name inside toolchain dir
    pub dir_key: &'static str,   // directory name under ~/.flux/toolchains/
}

/// All toolchains derived from VERSIONS.
pub fn all_toolchains() -> Vec<Toolchain> {
    VERSIONS
        .iter()
        .map(|(lang, version)| {
            let (binary, dir_key) = binary_for(*lang);
            Toolchain { lang, version, binary, dir_key }
        })
        .collect()
}

fn binary_for(lang: &str) -> (&'static str, &'static str) {
    match lang {
        "typescript"     => ("deno",    "deno"),
        "javascript"     => ("node",    "node"),
        "rust"           => ("cargo",   "rust"),
        "go"             => ("go",      "go"),
        "python"         => ("python3", "python"),
        "c" | "cpp"      => ("clang",   "wasi-sdk"),
        "zig"            => ("zig",     "zig"),
        "assemblyscript" => ("asc",     "assemblyscript"),
        "csharp"         => ("dotnet",  "dotnet"),
        "swift"          => ("swift",   "swift"),
        "kotlin"         => ("kotlinc", "kotlin"),
        "java"           => ("java",    "java"),
        "ruby"           => ("ruby",    "ruby"),
        _                => ("unknown", "unknown"),
    }
}

// ── Toolchain root ────────────────────────────────────────────────────────────

pub fn toolchain_root() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
        .join(".flux")
        .join("toolchains")
}

/// Returns the path to the toolchain binary if it is installed, else None.
pub fn toolchain_path(lang: &str) -> Option<std::path::PathBuf> {
    let (binary, dir_key) = binary_for(lang);
    let version = VERSIONS.iter()
        .find(|(l, _)| *l == lang)
        .map(|(_, v)| *v)?;

    // Version strings like "Deno 2.3" → "2.3"; "1.87.0 / edition 2021" → "1.87.0"
    let ver = version.split_whitespace()
        .find(|s| s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
        .unwrap_or(version);

    let path = toolchain_root()
        .join(dir_key)
        .join(ver)
        .join(binary);

    if path.exists() { Some(path) } else { None }
}

// ── Commands ──────────────────────────────────────────────────────────────────

#[derive(Debug, clap::Subcommand)]
pub enum ToolchainCommand {
    /// Show all languages, pinned versions, and install status
    List,
    /// Install toolchain for one language (or 'all')
    Install {
        /// Language name or 'all'
        #[arg(value_name = "LANG")]
        lang: String,
    },
    /// Print path to the binary for a language
    Which {
        #[arg(value_name = "LANG")]
        lang: String,
    },
}

pub async fn execute(cmd: ToolchainCommand) -> anyhow::Result<()> {
    match cmd {
        ToolchainCommand::List    => cmd_list(),
        ToolchainCommand::Install { lang } => cmd_install(&lang).await,
        ToolchainCommand::Which   { lang } => cmd_which(&lang),
    }
}

fn cmd_list() -> anyhow::Result<()> {
    println!();
    println!("{}", "  Flux toolchains".bold());
    println!();
    println!("  {:<16} {:<28} {}", "LANGUAGE".dimmed(), "VERSION".dimmed(), "STATUS".dimmed());
    println!("  {}", "─".repeat(60).dimmed());

    for tc in all_toolchains() {
        let installed = toolchain_path(tc.lang).is_some();
        let status = if installed {
            "✔ installed".green().to_string()
        } else {
            "○ not installed".dimmed().to_string()
        };
        println!("  {:<16} {:<28} {}", tc.lang, tc.version, status);
    }

    println!();
    println!("  Run {} to install a toolchain.", "flux toolchain install <lang>".cyan());
    println!();
    Ok(())
}

async fn cmd_install(lang: &str) -> anyhow::Result<()> {
    if lang == "all" {
        for tc in all_toolchains() {
            install_one(tc.lang).await?;
        }
    } else {
        install_one(lang).await?;
    }
    Ok(())
}

async fn install_one(lang: &str) -> anyhow::Result<()> {
    // Find version
    let version = VERSIONS.iter()
        .find(|(l, _)| *l == lang)
        .map(|(_, v)| *v)
        .ok_or_else(|| anyhow::anyhow!("Unknown language '{lang}'"))?;

    if toolchain_path(lang).is_some() {
        println!("  {} {} {} already installed", "✔".green().bold(), lang, version.dimmed());
        return Ok(());
    }

    println!("  {} Installing {} {}...", "↓".cyan().bold(), lang.bold(), version.dimmed());
    println!(
        "  {} Run: {}",
        "→".dimmed(),
        install_hint(lang, version).cyan()
    );
    println!(
        "  {} Automated download coming in a future release. Install manually for now.",
        "ℹ".yellow()
    );
    Ok(())
}

fn cmd_which(lang: &str) -> anyhow::Result<()> {
    match toolchain_path(lang) {
        Some(path) => {
            println!("{}", path.display());
            Ok(())
        }
        None => anyhow::bail!(
            "Toolchain for '{}' is not installed. Run: flux toolchain install {}",
            lang, lang
        ),
    }
}

/// Human-readable install hint for each language.
fn install_hint(lang: &str, version: &str) -> String {
    let ver = version.split_whitespace()
        .find(|s| s.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
        .unwrap_or(version);

    match lang {
        "typescript"     => format!("curl -fsSL https://deno.land/install.sh | sh -s v{ver}"),
        "javascript"     => format!("nvm install {ver}  # or: https://nodejs.org/en/download"),
        "rust"           => "rustup target add wasm32-wasip1".to_string(),
        "go"             => format!("https://go.dev/dl/go{ver}.{os}-{arch}.tar.gz",
                                     os = std::env::consts::OS,
                                     arch = std::env::consts::ARCH),
        "python"         => format!("pyenv install {ver}"),
        "c" | "cpp"      => format!("https://github.com/WebAssembly/wasi-sdk/releases/tag/wasi-sdk-24"),
        "zig"            => format!("https://ziglang.org/download/{ver}/zig-{os}-{arch}-{ver}.tar.xz",
                                     os = std::env::consts::OS,
                                     arch = std::env::consts::ARCH),
        "assemblyscript" => "npm install -g assemblyscript".to_string(),
        "csharp"         => format!("https://dotnet.microsoft.com/download/dotnet/{ver}"),
        "swift"          => format!("https://swift.org/install/  # version {ver}"),
        "kotlin"         => "sdk install kotlin  # via SDKMAN".to_string(),
        "java"           => "sdk install java 21-graal  # via SDKMAN".to_string(),
        "ruby"           => format!("rbenv install {ver}"),
        _                => format!("# see docs for {lang}"),
    }
}
