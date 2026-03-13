//! `flux toolchain` — manage pinned language runtimes and compilers.
//!
//! ## Storage layout
//! ```
//! ~/.flux/toolchains/
//!   deno/2.3.0/deno
//!   node/22.14.0/bin/node
//!   go/1.24.2/bin/go
//!   python/3.12.9/bin/python3
//!   zig/0.13.0/zig
//!   wasi-sdk/24.0/bin/clang
//!   kotlin/2.1.0/bin/kotlinc
//!   java/21.0.6/bin/java
//! ```
//!
//! ## Commands
//!   flux toolchain list               — show languages, versions, install status
//!   flux toolchain install <lang>     — download + install one toolchain
//!   flux toolchain install all        — install all 14
//!   flux toolchain which <lang>       — print binary path (for scripting)

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _};
use colored::Colorize;

use crate::new_function::VERSIONS;

// ── Archive types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
enum Archive {
    Zip,
    TarGz,
    TarXz,
}

// ── Download spec ─────────────────────────────────────────────────────────────

/// Everything needed to download + install one toolchain.
struct Spec {
    /// Exact version string used in URLs.
    version: &'static str,
    /// Archive format.
    archive: Archive,
    /// Resolved download URL for this OS + arch.
    url: fn(os: Os, arch: Arch) -> Option<String>,
    /// Relative path inside the extracted archive to the main binary.
    binary_in_archive: fn(os: Os, arch: Arch, ver: &str) -> String,
    /// Binary name placed under `~/.flux/toolchains/<dir_key>/<version>/`.
    binary_name: &'static str,
}

// ── OS / Arch helpers ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug)]
enum Os { MacOs, Linux, Windows }

#[derive(Clone, Copy, Debug)]
enum Arch { X86_64, Aarch64 }

fn current_os() -> Os {
    match std::env::consts::OS {
        "macos"   => Os::MacOs,
        "windows" => Os::Windows,
        _         => Os::Linux,
    }
}

fn current_arch() -> Arch {
    match std::env::consts::ARCH {
        "aarch64" | "arm64" => Arch::Aarch64,
        _                   => Arch::X86_64,
    }
}

// ── Per-language download specs ───────────────────────────────────────────────
// Full resolved semver versions (separate from display versions in VERSIONS).

fn spec_for(lang: &str) -> Option<Spec> {
    match lang {
        // ── Deno (TypeScript) ─────────────────────────────────────────────────
        "typescript" => Some(Spec {
            version: "2.3.3",
            archive: Archive::Zip,
            url: |os, arch| {
                let target = match (os, arch) {
                    (Os::MacOs,   Arch::Aarch64) => "aarch64-apple-darwin",
                    (Os::MacOs,   Arch::X86_64)  => "x86_64-apple-darwin",
                    (Os::Linux,   Arch::X86_64)  => "x86_64-unknown-linux-gnu",
                    (Os::Linux,   Arch::Aarch64) => "aarch64-unknown-linux-gnu",
                    (Os::Windows, _)             => "x86_64-pc-windows-msvc",
                };
                Some(format!(
                    "https://github.com/denoland/deno/releases/download/v2.3.3/deno-{target}.zip"
                ))
            },
            binary_in_archive: |os, _, _| {
                match os { Os::Windows => "deno.exe".into(), _ => "deno".into() }
            },
            binary_name: "deno",
        }),

        // ── Node.js (JavaScript) ──────────────────────────────────────────────
        "javascript" => Some(Spec {
            version: "22.14.0",
            archive: Archive::TarGz,
            url: |os, arch| {
                let (os_s, arch_s) = match (os, arch) {
                    (Os::MacOs,  Arch::Aarch64) => ("darwin", "arm64"),
                    (Os::MacOs,  Arch::X86_64)  => ("darwin", "x64"),
                    (Os::Linux,  Arch::X86_64)  => ("linux",  "x64"),
                    (Os::Linux,  Arch::Aarch64) => ("linux",  "arm64"),
                    (Os::Windows, _)            => return None, // use installer
                };
                Some(format!(
                    "https://nodejs.org/dist/v22.14.0/node-v22.14.0-{os_s}-{arch_s}.tar.gz"
                ))
            },
            binary_in_archive: |_, _, _| "node-v22.14.0-{os}-{arch}/bin/node".into(),
            binary_name: "node",
        }),

        // ── Go ────────────────────────────────────────────────────────────────
        "go" => Some(Spec {
            version: "1.24.2",
            archive: Archive::TarGz,
            url: |os, arch| {
                let (os_s, arch_s) = match (os, arch) {
                    (Os::MacOs,  Arch::Aarch64) => ("darwin",  "arm64"),
                    (Os::MacOs,  Arch::X86_64)  => ("darwin",  "amd64"),
                    (Os::Linux,  Arch::X86_64)  => ("linux",   "amd64"),
                    (Os::Linux,  Arch::Aarch64) => ("linux",   "arm64"),
                    (Os::Windows, _)            => ("windows", "amd64"),
                };
                Some(format!("https://go.dev/dl/go1.24.2.{os_s}-{arch_s}.tar.gz"))
            },
            binary_in_archive: |os, _, _| {
                match os {
                    Os::Windows => "go/bin/go.exe".into(),
                    _           => "go/bin/go".into(),
                }
            },
            binary_name: "go",
        }),

        // ── Zig ───────────────────────────────────────────────────────────────
        "zig" => Some(Spec {
            version: "0.13.0",
            archive: Archive::TarXz,
            url: |os, arch| {
                let (os_s, arch_s) = match (os, arch) {
                    (Os::MacOs,  Arch::Aarch64) => ("macos",   "aarch64"),
                    (Os::MacOs,  Arch::X86_64)  => ("macos",   "x86_64"),
                    (Os::Linux,  Arch::X86_64)  => ("linux",   "x86_64"),
                    (Os::Linux,  Arch::Aarch64) => ("linux",   "aarch64"),
                    (Os::Windows, _)            => ("windows", "x86_64"),
                };
                Some(format!(
                    "https://ziglang.org/download/0.13.0/zig-{os_s}-{arch_s}-0.13.0.tar.xz"
                ))
            },
            binary_in_archive: |os, arch, _| {
                let (os_s, arch_s) = match (os, arch) {
                    (Os::MacOs,  Arch::Aarch64) => ("macos",   "aarch64"),
                    (Os::MacOs,  Arch::X86_64)  => ("macos",   "x86_64"),
                    (Os::Linux,  Arch::X86_64)  => ("linux",   "x86_64"),
                    (Os::Linux,  Arch::Aarch64) => ("linux",   "aarch64"),
                    (Os::Windows, _)            => ("windows", "x86_64"),
                };
                let ext = match os { Os::Windows => ".exe", _ => "" };
                format!("zig-{os_s}-{arch_s}-0.13.0/zig{ext}")
            },
            binary_name: "zig",
        }),

        // ── Python (python-build-standalone by Astral) ────────────────────────
        "python" => Some(Spec {
            version: "3.12.9",
            archive: Archive::TarGz,
            url: |os, arch| {
                let (arch_s, os_s) = match (os, arch) {
                    (Os::MacOs,  Arch::Aarch64) => ("aarch64", "apple-darwin"),
                    (Os::MacOs,  Arch::X86_64)  => ("x86_64",  "apple-darwin"),
                    (Os::Linux,  Arch::X86_64)  => ("x86_64",  "unknown-linux-gnu"),
                    (Os::Linux,  Arch::Aarch64) => ("aarch64", "unknown-linux-gnu"),
                    (Os::Windows, _)            => ("x86_64",  "pc-windows-msvc"),
                };
                let ext = match os { Os::Windows => ".tar.gz", _ => ".tar.gz" };
                let _ = ext;
                Some(format!(
                    "https://github.com/astral-sh/python-build-standalone/releases/download/20250317/cpython-3.12.9%2B20250317-{arch_s}-{os_s}-install_only.tar.gz"
                ))
            },
            binary_in_archive: |os, _, _| {
                match os {
                    Os::Windows => "python/python.exe".into(),
                    _           => "python/bin/python3".into(),
                }
            },
            binary_name: "python3",
        }),

        // ── wasi-sdk (C + C++) ────────────────────────────────────────────────
        "c" | "cpp" => Some(Spec {
            version: "24.0",
            archive: Archive::TarGz,
            url: |os, arch| {
                let (arch_s, os_s) = match (os, arch) {
                    (Os::MacOs,  _)            => ("arm64",  "macos"),
                    (Os::Linux,  Arch::X86_64) => ("x86_64", "linux"),
                    (Os::Linux,  Arch::Aarch64)=> ("arm64",  "linux"),
                    (Os::Windows, _)           => return None, // no wasi-sdk for windows
                };
                Some(format!(
                    "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-24/wasi-sdk-24.0-{arch_s}-{os_s}.tar.gz"
                ))
            },
            binary_in_archive: |_, _, _| "wasi-sdk-24.0/bin/clang".into(),
            binary_name: "clang",
        }),

        // ── AssemblyScript (needs Node first, installed via npm) ──────────────
        // ── Rust (via rustup) ─────────────────────────────────────────────────
        // ── .NET, Swift, Kotlin, Java, Ruby — complex installers ──────────────
        // These fall through to the manual-hint path.
        _ => None,
    }
}

// ── Toolchain root ────────────────────────────────────────────────────────────

pub fn toolchain_root() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".flux")
        .join("toolchains")
}

/// Key (directory name) under toolchain_root for a language.
fn dir_key(lang: &str) -> &'static str {
    match lang {
        "typescript"     => "deno",
        "javascript"     => "node",
        "rust"           => "rust",
        "go"             => "go",
        "python"         => "python",
        "c" | "cpp"      => "wasi-sdk",
        "zig"            => "zig",
        "assemblyscript" => "assemblyscript",
        "csharp"         => "dotnet",
        "swift"          => "swift",
        "kotlin"         => "kotlin",
        "java"           => "java",
        "ruby"           => "ruby",
        _                => "unknown",
    }
}

/// Binary name for a language.
pub fn binary_name(lang: &str) -> &'static str {
    match lang {
        "typescript"     => "deno",
        "javascript"     => "node",
        "rust"           => "cargo",
        "go"             => "go",
        "python"         => "python3",
        "c" | "cpp"      => "clang",
        "zig"            => "zig",
        "assemblyscript" => "asc",
        "csharp"         => "dotnet",
        "swift"          => "swift",
        "kotlin"         => "kotlinc",
        "java"           => "java",
        "ruby"           => "ruby",
        _                => "unknown",
    }
}

/// Resolved full version for a language (for directory naming).
fn resolved_version(lang: &str) -> &'static str {
    match spec_for(lang) {
        Some(s) => s.version,
        None    => VERSIONS.iter()
            .find(|(l, _)| *l == lang)
            .map(|(_, v)| *v)
            .unwrap_or("unknown"),
    }
}

/// Returns the installed binary path, or None if not installed.
pub fn toolchain_path(lang: &str) -> Option<PathBuf> {
    // First check ~/.flux/toolchains/<dir_key>/<version>/<binary>
    let bin = binary_name(lang);
    let ver = resolved_version(lang);
    let managed = toolchain_root()
        .join(dir_key(lang))
        .join(ver)
        .join(bin);
    if managed.exists() {
        return Some(managed);
    }

    // Fall back to system PATH
    which::which(bin).ok()
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
        ToolchainCommand::List             => cmd_list(),
        ToolchainCommand::Install { lang } => cmd_install(&lang).await,
        ToolchainCommand::Which   { lang } => cmd_which(&lang),
    }
}

// ── list ──────────────────────────────────────────────────────────────────────

fn cmd_list() -> anyhow::Result<()> {
    println!();
    println!("{}", "  Flux toolchains".bold());
    println!();
    println!("  {:<16} {:<28} {:<12} {}",
        "LANGUAGE".dimmed(), "PINNED VERSION".dimmed(),
        "MANAGED".dimmed(), "PATH".dimmed());
    println!("  {}", "─".repeat(72).dimmed());

    for (lang, display_ver) in VERSIONS {
        let path = toolchain_path(lang);
        let managed_path = {
            let p = toolchain_root()
                .join(dir_key(lang))
                .join(resolved_version(lang))
                .join(binary_name(lang));
            p.exists()
        };

        let (status, path_str) = match &path {
            Some(p) if managed_path => (
                "✔ managed".green().to_string(),
                p.display().to_string().dimmed().to_string(),
            ),
            Some(p) => (
                "~ system".yellow().to_string(),
                p.display().to_string().dimmed().to_string(),
            ),
            None => (
                "○ missing".dimmed().to_string(),
                "not found".dimmed().to_string(),
            ),
        };

        println!("  {:<16} {:<28} {:<20} {}",
            lang, display_ver, status, path_str);
    }

    println!();
    println!("  {}  managed = downloaded by Flux to ~/.flux/toolchains/", "legend:".dimmed());
    println!("  {}  system  = found on PATH (not version-managed)", "       ".dimmed());
    println!();
    println!("  Run {} to install.", "flux toolchain install <lang|all>".cyan());
    println!();
    Ok(())
}

// ── which ─────────────────────────────────────────────────────────────────────

fn cmd_which(lang: &str) -> anyhow::Result<()> {
    match toolchain_path(lang) {
        Some(p) => { println!("{}", p.display()); Ok(()) }
        None    => bail!(
            "Toolchain for '{}' not installed. Run: flux toolchain install {}",
            lang, lang
        ),
    }
}

// ── install ───────────────────────────────────────────────────────────────────

pub async fn cmd_install(lang: &str) -> anyhow::Result<()> {
    let all: Vec<&str> = VERSIONS.iter().map(|(l, _)| *l).collect();
    let targets: Vec<&str> = if lang == "all" { all.clone() } else { vec![lang] };

    for target in targets {
        install_one(target).await?;
    }
    Ok(())
}

async fn install_one(lang: &str) -> anyhow::Result<()> {
    // Already managed?
    let managed = toolchain_root()
        .join(dir_key(lang))
        .join(resolved_version(lang))
        .join(binary_name(lang));
    if managed.exists() {
        let ver = resolved_version(lang);
        println!("  {} {} {} already installed at {}",
            "✔".green().bold(), lang.bold(), ver.dimmed(),
            managed.display().to_string().dimmed());
        return Ok(());
    }

    // Get download spec
    let spec = match spec_for(lang) {
        Some(s) => s,
        None    => return install_manual_hint(lang),
    };

    let os   = current_os();
    let arch = current_arch();

    let url = match (spec.url)(os, arch) {
        Some(u) => u,
        None    => return install_manual_hint(lang),
    };

    let dest_dir = toolchain_root()
        .join(dir_key(lang))
        .join(spec.version);
    std::fs::create_dir_all(&dest_dir)
        .with_context(|| format!("Cannot create {}", dest_dir.display()))?;

    println!();
    println!("  {} {} {}", "↓".cyan().bold(), lang.bold(), spec.version.dimmed());
    println!("  {} {}", "  ".dimmed(), url.dimmed());

    // Download to temp file
    let tmp_path = dest_dir.join(format!("_download.{}", archive_ext(spec.archive)));
    download_with_progress(&url, &tmp_path).await
        .with_context(|| format!("Failed to download {lang}"))?;

    // Extract
    let bin_in_archive = (spec.binary_in_archive)(os, arch, spec.version);
    print!("  {} Extracting...", "▸".dimmed());
    std::io::stdout().flush().ok();

    extract_binary(&tmp_path, spec.archive, &bin_in_archive, &dest_dir, spec.binary_name)
        .with_context(|| format!("Failed to extract {lang}"))?;

    std::fs::remove_file(&tmp_path).ok();

    // Verify
    let final_bin = dest_dir.join(spec.binary_name);
    if !final_bin.exists() {
        bail!("Binary not found after extraction: {}", final_bin.display());
    }

    // Make executable on Unix
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&final_bin)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&final_bin, perms)?;
    }

    println!("\r  {} {} installed → {}", "✔".green().bold(), lang.bold(),
        final_bin.display().to_string().cyan());
    Ok(())
}

fn install_manual_hint(lang: &str) -> anyhow::Result<()> {
    let hint = match lang {
        "rust"           => "rustup target add wasm32-wasip1  # https://rustup.rs",
        "assemblyscript" => "npm install -g assemblyscript  # requires node first",
        "csharp"         => "https://dotnet.microsoft.com/download/dotnet/9.0",
        "swift"          => "https://www.swift.org/install/  # or: xcode-select --install",
        "kotlin"         => "sdk install kotlin  # https://sdkman.io",
        "java"           => "sdk install java 21-tem  # https://sdkman.io",
        "ruby"           => "rbenv install 3.3.0  # https://github.com/rbenv/rbenv",
        _                => "See https://fluxbase.dev/docs/toolchains",
    };
    println!("  {} {} requires manual install:", "ℹ".yellow().bold(), lang.bold());
    println!("    {}", hint.cyan());
    Ok(())
}

// ── Download ──────────────────────────────────────────────────────────────────

async fn download_with_progress(url: &str, dest: &Path) -> anyhow::Result<()> {
    use tokio::io::AsyncWriteExt;

    let resp = reqwest::get(url).await
        .with_context(|| format!("GET {url}"))?;

    if !resp.status().is_success() {
        bail!("HTTP {} from {url}", resp.status());
    }

    let total = resp.content_length();
    let mut downloaded: u64 = 0;
    let mut file = tokio::fs::File::create(dest).await
        .with_context(|| format!("Cannot create {}", dest.display()))?;

    let mut stream = resp.bytes_stream();
    use futures_util::StreamExt;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("Download stream error")?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if let Some(t) = total {
            let pct = downloaded * 100 / t;
            print!("\r  {} Downloading... {}%    ", "↓".cyan(), pct);
        } else {
            print!("\r  {} Downloading... {}KB    ", "↓".cyan(), downloaded / 1024);
        }
        std::io::stdout().flush().ok();
    }
    file.flush().await?;
    println!("\r  {} Downloaded {}KB    ", "✔".green(), downloaded / 1024);
    Ok(())
}

// ── Extraction ────────────────────────────────────────────────────────────────

fn archive_ext(a: Archive) -> &'static str {
    match a {
        Archive::Zip   => "zip",
        Archive::TarGz => "tar.gz",
        Archive::TarXz => "tar.xz",
    }
}

/// Extract a single named binary from an archive into dest_dir, renaming it to `out_name`.
fn extract_binary(
    archive_path: &Path,
    archive_type: Archive,
    binary_path_in_archive: &str,
    dest_dir: &Path,
    out_name: &str,
) -> anyhow::Result<()> {
    let out_path = dest_dir.join(out_name);

    match archive_type {
        Archive::Zip => extract_from_zip(archive_path, binary_path_in_archive, &out_path),
        Archive::TarGz => extract_from_tar_gz(archive_path, binary_path_in_archive, &out_path),
        Archive::TarXz => extract_from_tar_xz(archive_path, binary_path_in_archive, &out_path),
    }
}

fn extract_from_zip(zip_path: &Path, entry: &str, out: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::open(zip_path)?;
    let mut archive = zip::ZipArchive::new(file)?;

    // Try exact name, then just filename match
    let idx = (0..archive.len()).find(|&i| {
        archive.by_index(i).ok()
            .map(|e| e.name() == entry || e.name().ends_with(&format!("/{entry}"))
                     || e.name() == out.file_name().unwrap_or_default().to_str().unwrap_or(""))
            .unwrap_or(false)
    }).with_context(|| format!("'{entry}' not found in zip"))?;

    let mut zip_entry = archive.by_index(idx)?;
    let mut buf = Vec::new();
    zip_entry.read_to_end(&mut buf)?;
    std::fs::write(out, buf)?;
    Ok(())
}

fn extract_from_tar_gz(tar_path: &Path, entry: &str, out: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::open(tar_path)?;
    let gz   = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(gz);
    extract_entry_from_tar(&mut archive, entry, out)
}

fn extract_from_tar_xz(tar_path: &Path, entry: &str, out: &Path) -> anyhow::Result<()> {
    let file = std::fs::File::open(tar_path)?;
    let xz   = xz2::read::XzDecoder::new(file);
    let mut archive = tar::Archive::new(xz);
    extract_entry_from_tar(&mut archive, entry, out)
}

fn extract_entry_from_tar<R: Read>(
    archive: &mut tar::Archive<R>,
    entry: &str,
    out: &Path,
) -> anyhow::Result<()> {
    for file in archive.entries()? {
        let mut file = file?;
        let path = file.path()?.to_path_buf();
        let path_str = path.to_string_lossy();

        if path_str == entry
            || path_str.ends_with(&format!("/{entry}"))
            || path.file_name().map(|n| n.to_string_lossy().to_string())
                   == out.file_name().map(|n| n.to_string_lossy().to_string())
        {
            let mut buf = Vec::new();
            file.read_to_end(&mut buf)?;
            std::fs::write(out, buf)?;
            return Ok(());
        }
    }
    bail!("'{entry}' not found in tar archive")
}
