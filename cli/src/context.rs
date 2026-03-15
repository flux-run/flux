//! Flux context management — named connections to Flux server instances.
//!
//! Works exactly like `kubectl config` / AWS CLI profiles. Contexts are stored
//! in `~/.flux/contexts.toml` (global) and can be overridden per-project with
//! `.flux/contexts.toml` (local).
//!
//! ## Commands
//!
//! ```text
//! flux link prod https://myapp.com --key sk_live_xxx   # add / update context
//! flux use  prod                                        # set active context
//! flux context                                          # show current context
//! flux unlink staging                                   # remove context
//! ```
//!
//! ## Resolution order for every command that talks to a Flux server
//!
//! 1. `--context <name>` CLI flag
//! 2. `FLUX_CONTEXT` env var
//! 3. `.flux/contexts.toml` in project root   (per-project override)
//! 4. `~/.flux/contexts.toml` active context  (global)
//! 5. `http://localhost:4000`                 (zero-config local default)
//!
//! ## SOLID
//!
//! - SRP: `ContextStore` only reads/writes config; resolution logic is in
//!   `resolve_context()`.
//! - OCP: New context fields (e.g. tls_ca_cert) can be added to `Context`
//!   without changing any call-site.
//! - DIP: All commands call `resolve_context()` — they never read config files
//!   directly.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use crate::config::{DEFAULT_SERVER_PORT, local_url};

use anyhow::{Context as _, bail};
use colored::Colorize;
use serde::{Deserialize, Serialize};

// ── Types ─────────────────────────────────────────────────────────────────────

/// A single named connection to a Flux server.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FluxContext {
    /// Base URL of the Flux server, e.g. `https://myapp.com` or
    /// `http://localhost:4000`.
    pub endpoint: String,

    /// API key for authenticating against the remote server.
    /// Empty string means local dev mode (no auth).
    #[serde(default)]
    pub api_key: String,
}

/// Contents of a `contexts.toml` file — either global or per-project.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ContextStore {
    /// Name of the currently active context.
    #[serde(default = "default_active")]
    pub active: String,

    /// Named contexts.
    #[serde(default)]
    pub contexts: HashMap<String, FluxContext>,
}

fn default_active() -> String {
    "local".into()
}

impl ContextStore {
    fn local_default() -> Self {
        let mut s = Self::default();
        s.contexts.insert(
            "local".into(),
            FluxContext {
                endpoint: local_url(DEFAULT_SERVER_PORT),
                api_key:  String::new(),
            },
        );
        s
    }
}

// ── I/O ───────────────────────────────────────────────────────────────────────

/// Path to the global contexts file: `~/.flux/contexts.toml`.
fn global_contexts_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".flux")
        .join("contexts.toml")
}

/// Read a `ContextStore` from `path`. Returns the empty default if the file
/// does not exist yet (first run).
fn read_store(path: &Path) -> anyhow::Result<ContextStore> {
    if !path.exists() {
        return Ok(ContextStore::local_default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let store: ContextStore = toml::from_str(&raw)
        .with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(store)
}

fn write_store(path: &Path, store: &ContextStore) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let toml_str = toml::to_string_pretty(store)
        .context("Failed to serialize context store")?;
    std::fs::write(path, toml_str)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

// ── Resolution ────────────────────────────────────────────────────────────────

/// Resolved context passed to every command that talks to a Flux server.
#[derive(Debug, Clone)]
pub struct ResolvedContext {
    /// The name used to identify this context (e.g. "prod", "local").
    pub name: String,
    pub endpoint: String,
    pub api_key: String,
}

impl ResolvedContext {
    /// Returns `true` if this context has no API key — treat as local dev.
    pub fn is_local(&self) -> bool {
        self.api_key.is_empty()
    }
}

/// Resolve the active context following the precedence chain:
///
/// 1. `explicit` — from a `--context` CLI flag (pass `None` if absent)
/// 2. `FLUX_CONTEXT` env var
/// 3. `.flux/contexts.toml` in `project_root`
/// 4. `~/.flux/contexts.toml`
/// 5. Hard-coded local default
pub fn resolve_context(
    explicit: Option<&str>,
    project_root: Option<&Path>,
) -> anyhow::Result<ResolvedContext> {
    // 1. CLI flag
    let name_override = explicit
        .map(ToOwned::to_owned)
        // 2. env var
        .or_else(|| std::env::var("FLUX_CONTEXT").ok());

    // 3. Per-project override
    let project_store = project_root
        .map(|r| r.join(".flux").join("contexts.toml"))
        .filter(|p| p.exists())
        .map(|p| read_store(&p))
        .transpose()?;

    // 4. Global store
    let global_store = read_store(&global_contexts_path())?;

    // Pick which store to consult, and which context name to look up.
    let name = name_override
        .as_deref()
        .or_else(|| project_store.as_ref().map(|s| s.active.as_str()))
        .unwrap_or(&global_store.active)
        .to_owned();

    // Search project store first, then global.
    let ctx = project_store
        .as_ref()
        .and_then(|s| s.contexts.get(&name))
        .or_else(|| global_store.contexts.get(&name))
        // 5. fallback
        .cloned()
        .unwrap_or(FluxContext {
            endpoint: local_url(DEFAULT_SERVER_PORT),
            api_key:  String::new(),
        });

    Ok(ResolvedContext {
        name,
        endpoint: ctx.endpoint.trim_end_matches('/').to_owned(),
        api_key:  ctx.api_key,
    })
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// `flux link <name> <endpoint> [--key <api_key>]`
///
/// Add or update a named context in the global config.
pub fn execute_link(name: String, endpoint: String, key: Option<String>) -> anyhow::Result<()> {
    let path = global_contexts_path();
    let mut store = read_store(&path)?;

    let api_key = key.unwrap_or_default();
    let is_update = store.contexts.contains_key(&name);

    store.contexts.insert(
        name.clone(),
        FluxContext {
            endpoint: endpoint.clone(),
            api_key:  api_key.clone(),
        },
    );

    write_store(&path, &store)?;

    let verb = if is_update { "Updated" } else { "Linked" };
    println!(
        "{} context {} → {}{}",
        verb.green().bold(),
        name.cyan().bold(),
        endpoint.cyan(),
        if api_key.is_empty() { "" } else { "  (key set)" },
    );
    Ok(())
}

/// `flux use <name>`
///
/// Set the active context in the global config.
pub fn execute_use(name: String) -> anyhow::Result<()> {
    let path = global_contexts_path();
    let mut store = read_store(&path)?;

    if !store.contexts.contains_key(&name) {
        bail!(
            "Context '{}' not found. Run `flux link {} <endpoint>` to create it.",
            name,
            name
        );
    }

    store.active = name.clone();
    write_store(&path, &store)?;

    let ctx = store.contexts.get(&name).unwrap();
    println!(
        "{} Switched to context {}  ({})",
        "✔".green().bold(),
        name.cyan().bold(),
        ctx.endpoint.dimmed(),
    );
    Ok(())
}

/// `flux context`
///
/// Print the currently active context and all known contexts.
pub fn execute_context(project_root: Option<&Path>) -> anyhow::Result<()> {
    let resolved = resolve_context(None, project_root)?;
    let global_store = read_store(&global_contexts_path())?;

    println!();
    println!(
        "  {} {}  {}",
        "Active context:".bold(),
        resolved.name.cyan().bold(),
        resolved.endpoint.dimmed(),
    );
    if resolved.is_local() {
        println!("  {}", "(local dev — no API key required)".dimmed());
    } else {
        println!(
            "  {} {}",
            "API key:".dimmed(),
            mask_key(&resolved.api_key).dimmed()
        );
    }
    println!();
    println!("  {}", "All contexts:".bold());
    for (name, ctx) in &global_store.contexts {
        let marker = if *name == global_store.active { "●" } else { " " };
        println!(
            "    {} {}  {}",
            marker.green(),
            name.cyan(),
            ctx.endpoint.dimmed()
        );
    }
    println!();
    Ok(())
}

/// `flux unlink <name>`
///
/// Remove a named context from the global config.
pub fn execute_unlink(name: String) -> anyhow::Result<()> {
    let path = global_contexts_path();
    let mut store = read_store(&path)?;

    if !store.contexts.contains_key(&name) {
        bail!("Context '{}' not found.", name);
    }
    if store.active == name {
        bail!(
            "Cannot unlink the active context '{}'. Run `flux use <other>` first.",
            name
        );
    }

    store.contexts.remove(&name);
    write_store(&path, &store)?;

    println!("{} Unlinked context {}", "✔".green().bold(), name.cyan().bold());
    Ok(())
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn mask_key(key: &str) -> String {
    if key.len() <= 8 {
        return "****".into();
    }
    format!("{}…{}", &key[..4], &key[key.len() - 4..])
}
