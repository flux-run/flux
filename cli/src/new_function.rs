//! `flux function create <name> [--language <lang>]`
//!
//! Scaffolds a new function inside `functions/` from the `scaffolds/functions/<lang>/`
//! directory embedded into the binary at compile time via `include_str!()`.
//!
//! All scaffold file contents have `{name}` / `{Name}` as the function name placeholder,
//! substituted at runtime.
//!
//! Supported languages:
//!   typescript    Deno V8 (JS/TS native)
//!   javascript    Deno V8 (JS/TS native)

use std::path::Path;

use anyhow::{bail, Context as _};
use colored::Colorize;

// ── Pinned language versions (shown in `flux function create --help`) ──────────

pub const VERSIONS: &[(&str, &str)] = &[
    ("typescript", "Deno V8 (JS/TS native)"),
    ("javascript", "Deno V8 (JS/TS native)"),
];

// ── Supported languages ───────────────────────────────────────────────────────

const LANGUAGES: &[(&str, &[&str])] = &[
    ("typescript", &["ts", "deno"]),
    ("javascript", &["js", "node"]),
];

fn resolve_language(input: &str) -> anyhow::Result<&'static str> {
    let lower = input.to_lowercase();
    for (canonical, aliases) in LANGUAGES {
        if *canonical == lower || aliases.contains(&lower.as_str()) {
            return Ok(canonical);
        }
    }
    let valid: Vec<&str> = LANGUAGES.iter().map(|(c, _)| *c).collect();
    bail!(
        "Unknown language '{}'. Valid options:\n  {}",
        input,
        valid.join(", ")
    )
}

// ── Scaffold files embedded at compile time ───────────────────────────────────
// Each language returns a list of (relative_path, content) pairs.
// {name} in content is replaced with the actual function name at runtime.

fn scaffold_files(lang: &str) -> Vec<(&'static str, &'static str)> {
    match lang {
        "typescript" => vec![
            ("index.ts",   include_str!("../../scaffolds/functions/typescript/index.ts")),
            ("flux.json",  include_str!("../../scaffolds/functions/typescript/flux.json")),
            ("deno.json",  include_str!("../../scaffolds/functions/typescript/deno.json")),
        ],
        "javascript" => vec![
            ("index.js",      include_str!("../../scaffolds/functions/javascript/index.js")),
            ("flux.json",     include_str!("../../scaffolds/functions/javascript/flux.json")),
            ("package.json",  include_str!("../../scaffolds/functions/javascript/package.json")),
        ],
        _ => vec![],
    }
}

// ── Name substitution ─────────────────────────────────────────────────────────

fn substitute(content: &str, name: &str) -> String {
    let pascal = to_pascal_case(name);
    content
        .replace("{name}", name)
        .replace("{Name}", &pascal)
        .replace("{NAME}", &name.to_uppercase())
}

fn to_pascal_case(s: &str) -> String {
    s.split(['_', '-'])
        .map(|word| {
            let mut c = word.chars();
            match c.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub fn execute_new_function(name: String, language: Option<String>) -> anyhow::Result<()> {
    if name.is_empty() || !name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        bail!("Function name must contain only letters, digits, underscores, and hyphens.");
    }

    let lang = resolve_language(language.as_deref().unwrap_or("typescript"))?;

    let project_root = crate::dev::find_project_root_pub()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let fn_dir = project_root.join("functions").join(&name);

    if fn_dir.exists() {
        bail!(
            "functions/{} already exists. Choose a different name or delete the directory first.",
            name
        );
    }

    println!();
    println!(
        "{} Scaffolding function {} ({})",
        "◆".cyan().bold(),
        name.cyan().bold(),
        lang.dimmed()
    );
    println!();

    std::fs::create_dir_all(&fn_dir)
        .with_context(|| format!("Failed to create {}", fn_dir.display()))?;

    let files = scaffold_files(lang);
    for (rel_path, content) in &files {
        let substituted = substitute(content, &name);
        write_file(&fn_dir, rel_path, &substituted)?;
        println!("  {} functions/{}/{}", "✔".green().bold(), name, rel_path);
    }

    println!();
    println!("  {}", "Next steps:".bold());
    let main_file = files.first().map(|(p, _)| *p).unwrap_or("index.ts");
    println!(
        "    1.  Edit {}",
        format!("functions/{}/{}", name, main_file).cyan()
    );
    println!("    2.  {}", "flux deploy".cyan());
    println!();

    Ok(())
}

fn write_file(dir: &Path, filename: &str, content: &str) -> anyhow::Result<()> {
    let path = dir.join(filename);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))
}
