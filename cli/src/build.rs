use std::collections::HashSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use serde::{Deserialize, Serialize};

// ─── CLI args ────────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct BuildArgs {
    /// Entry file (JS or TS) to analyse and bundle.
    #[arg(value_name = "ENTRY", default_value = "index.ts")]
    pub entry: String,

    /// Skip the optional esbuild bundling step.
    #[arg(long)]
    pub no_bundle: bool,

    /// Skip minification when bundling with esbuild.
    #[arg(long)]
    pub no_minify: bool,
}

// ─── Types ───────────────────────────────────────────────────────────────────

/// Runtime capability categories. Serialised as snake_case inside `flux.json`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFeature {
    /// TextEncoder/Decoder, streams, AbortController — always included.
    Web,
    /// fetch(), Headers, Request, Response — always included.
    Fetch,
    /// SubtleCrypto, randomUUID — always included.
    Crypto,
    /// WebSocket client.
    Websocket,
    /// require(), node:* modules, bare npm imports.
    Node,
    /// node:fs, Deno.readFile / writeFile.
    Fs,
    /// node:net, Deno.connect / listen.
    Net,
    /// Deno.env, process.env, node:os.
    Os,
    /// Deno.Command, node:child_process.
    Process,
}

/// Written to `flux.json` at the project root after `flux build`.
#[derive(Debug, Serialize, Deserialize)]
pub struct FluxManifest {
    pub flux_version: String,
    pub entry: String,
    pub code_hash: String,
    pub built_at: String,
    pub runtime_features: Vec<RuntimeFeature>,
    /// Path to the esbuild bundle if one was produced.
    pub bundled: Option<String>,
    pub minified: bool,
}

// ─── Entry point ─────────────────────────────────────────────────────────────

pub async fn execute(args: BuildArgs) -> Result<()> {
    let entry = PathBuf::from(&args.entry);
    if !entry.exists() {
        bail!("entry file not found: {}", entry.display());
    }

    let source = std::fs::read_to_string(&entry)
        .with_context(|| format!("failed to read {}", entry.display()))?;

    let features = detect_features(&source);
    let code_hash = content_hash(&source);

    let bundled = if args.no_bundle {
        None
    } else {
        try_bundle(&entry, !args.no_minify).await
    };

    let manifest = FluxManifest {
        flux_version: "0.2".to_string(),
        entry: args.entry.clone(),
        code_hash: code_hash.clone(),
        built_at: chrono::Utc::now().to_rfc3339(),
        runtime_features: features.iter().cloned().collect(),
        minified: !args.no_minify && bundled.is_some(),
        bundled: bundled.clone(),
    };

    let manifest_json = serde_json::to_string_pretty(&manifest)
        .context("failed to serialise flux.json")?;

    let out_path = entry
        .parent()
        .unwrap_or(Path::new("."))
        .join("flux.json");

    std::fs::write(&out_path, &manifest_json)
        .with_context(|| format!("failed to write {}", out_path.display()))?;

    // Print a concise build report.
    println!("built    {}", entry.display());
    println!("hash     {}", &code_hash[..12]);
    println!(
        "features {}",
        features
            .iter()
            .map(feature_name)
            .collect::<Vec<_>>()
            .join(", ")
    );
    match &bundled {
        Some(b) => println!("bundle   {b}"),
        None if !args.no_bundle => println!("bundle   skipped (esbuild not found)"),
        None => {}
    }
    println!("wrote    {}", out_path.display());

    Ok(())
}

// ─── Import analysis ─────────────────────────────────────────────────────────

/// Analyse `source` text and return the set of runtime features it requires.
pub fn detect_features(source: &str) -> HashSet<RuntimeFeature> {
    let mut f = HashSet::new();

    // Baseline — always present.
    f.insert(RuntimeFeature::Web);
    f.insert(RuntimeFeature::Fetch);
    f.insert(RuntimeFeature::Crypto);

    if source.contains("WebSocket") {
        f.insert(RuntimeFeature::Websocket);
    }

    if source.contains("require(")
        || source.contains("from 'node:")
        || source.contains("from \"node:")
        || source.contains("from 'npm:")
        || source.contains("from \"npm:")
        || has_npm_import(source)
    {
        f.insert(RuntimeFeature::Node);
    }

    if source.contains("Deno.readFile")
        || source.contains("Deno.writeFile")
        || source.contains("Deno.open")
        || source.contains("'node:fs'")
        || source.contains("\"node:fs\"")
        || source.contains("from 'fs'")
        || source.contains("from \"fs\"")
        || source.contains("require('fs')")
        || source.contains("require(\"fs\")")
    {
        f.insert(RuntimeFeature::Fs);
    }

    if source.contains("Deno.connect")
        || source.contains("Deno.listen")
        || source.contains("'node:net'")
        || source.contains("\"node:net\"")
        || source.contains("'node:dgram'")
        || source.contains("\"node:dgram\"")
        || source.contains("from 'net'")
        || source.contains("from \"net\"")
    {
        f.insert(RuntimeFeature::Net);
    }

    if source.contains("Deno.env")
        || source.contains("process.env")
        || source.contains("'node:os'")
        || source.contains("\"node:os\"")
        || source.contains("from 'os'")
        || source.contains("from \"os\"")
    {
        f.insert(RuntimeFeature::Os);
    }

    if source.contains("Deno.Command")
        || source.contains("child_process")
        || source.contains("'node:child_process'")
        || source.contains("\"node:child_process\"")
    {
        f.insert(RuntimeFeature::Process);
    }

    f
}

/// Returns true if any `import … from` line names a bare npm package (not a
/// relative path, URL, `node:`, `npm:`, or `jsr:` specifier).
fn has_npm_import(source: &str) -> bool {
    for line in source.lines() {
        let line = line.trim();
        if !line.starts_with("import ") {
            continue;
        }
        for (quote, close) in [("from '", '\''), ("from \"", '"')] {
            if let Some(idx) = line.rfind(quote) {
                let rest = &line[idx + quote.len()..];
                if let Some(end) = rest.find(close) {
                    let pkg = &rest[..end];
                    if !pkg.is_empty()
                        && !pkg.starts_with('.')
                        && !pkg.starts_with('/')
                        && !pkg.starts_with("node:")
                        && !pkg.starts_with("npm:")
                        && !pkg.starts_with("jsr:")
                        && !pkg.starts_with("https:")
                        && !pkg.starts_with("http:")
                    {
                        return true;
                    }
                }
            }
        }
    }
    false
}

// ─── Hashing ─────────────────────────────────────────────────────────────────

/// Fast, non-cryptographic content fingerprint for change detection.
pub fn content_hash(source: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    source.hash(&mut h);
    format!("{:016x}", h.finish())
}

// ─── esbuild bundling (optional) ─────────────────────────────────────────────

/// Try to bundle `entry` with esbuild if it is on PATH.
/// Returns the output file path on success, `None` if esbuild is unavailable.
async fn try_bundle(entry: &PathBuf, minify: bool) -> Option<String> {
    let ok = tokio::process::Command::new("which")
        .arg("esbuild")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        return None;
    }

    let out_dir = entry.parent().unwrap_or(Path::new(".")).join(".flux");
    std::fs::create_dir_all(&out_dir).ok()?;
    let out_file = out_dir.join("bundle.js");

    let mut cmd = tokio::process::Command::new("esbuild");
    cmd.arg(entry)
        .arg("--bundle")
        .arg("--platform=node")
        .arg("--format=esm")
        .arg(format!("--outfile={}", out_file.display()));
    if minify {
        cmd.arg("--minify");
    }

    let status = cmd.status().await.ok()?;
    if status.success() {
        Some(out_file.to_string_lossy().into_owned())
    } else {
        None
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn feature_name(f: &RuntimeFeature) -> &'static str {
    match f {
        RuntimeFeature::Web => "web",
        RuntimeFeature::Fetch => "fetch",
        RuntimeFeature::Crypto => "crypto",
        RuntimeFeature::Websocket => "websocket",
        RuntimeFeature::Node => "node",
        RuntimeFeature::Fs => "fs",
        RuntimeFeature::Net => "net",
        RuntimeFeature::Os => "os",
        RuntimeFeature::Process => "process",
    }
}
