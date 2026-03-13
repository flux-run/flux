//! `flux function create <name> [--language <lang>]`
//!
//! Scaffolds a new function inside `functions/` from the `scaffolds/functions/<lang>/`
//! directory embedded into the binary at compile time via `include_str!()`.
//!
//! All scaffold file contents have `{name}` / `{Name}` as the function name placeholder,
//! substituted at runtime.
//!
//! Supported languages — pinned versions:
//!   typescript    Deno 2.3
//!   javascript    Node.js 22 LTS
//!   rust          1.87.0 / edition 2021
//!   go            1.24
//!   python        3.12
//!   c / cpp       wasi-sdk 24 / clang 17
//!   zig           0.13.0
//!   assemblyscript 0.27.x
//!   csharp        .NET 9
//!   swift         6.0
//!   kotlin        2.1 / Gradle 8.10
//!   java          21 LTS / Gradle 8.10
//!   ruby          3.3

use std::path::Path;

use anyhow::{bail, Context as _};
use colored::Colorize;

// ── Pinned language versions (shown in `flux function create --help`) ──────────

pub const VERSIONS: &[(&str, &str)] = &[
    ("typescript",     "Deno 2.3"),
    ("javascript",     "Node.js 22 LTS"),
    ("rust",           "1.87.0 / edition 2021"),
    ("go",             "1.24"),
    ("python",         "3.12"),
    ("c",              "wasi-sdk 24 / clang 17"),
    ("cpp",            "wasi-sdk 24 / clang 17"),
    ("zig",            "0.13.0"),
    ("assemblyscript", "0.27.x"),
    ("csharp",         ".NET 9"),
    ("swift",          "6.0"),
    ("kotlin",         "2.1 / Gradle 8.10"),
    ("java",           "21 LTS / Gradle 8.10"),
    ("ruby",           "3.3"),
];

// ── Per-language build/run actions ────────────────────────────────────────────

pub const ACTIONS: &[(&str, &str, &str)] = &[
    // (language, check/build cmd, run cmd)
    ("typescript",     "deno check index.ts",                    "deno run index.ts"),
    ("javascript",     "node --check index.js",                  "node index.js"),
    ("rust",           "cargo check --target wasm32-wasip1",     "cargo build --release --target wasm32-wasip1"),
    ("go",             "GOOS=wasip1 GOARCH=wasm go build .",     "GOOS=wasip1 GOARCH=wasm go build ."),
    ("python",         "python3 -m py_compile handler.py",       "python3 handler.py"),
    ("c",              "make check",                             "make"),
    ("cpp",            "make check",                             "make"),
    ("zig",            "zig build",                              "zig build"),
    ("assemblyscript", "npx asc index.ts",                       "npx asc index.ts"),
    ("csharp",         "dotnet build",                           "dotnet build"),
    ("swift",          "swift build",                            "swift build"),
    ("kotlin",         "./gradlew build",                        "./gradlew build"),
    ("java",           "./gradlew build",                        "./gradlew build"),
    ("ruby",           "ruby -c handler.rb",                     "ruby handler.rb"),
];

// ── Supported languages ───────────────────────────────────────────────────────

const LANGUAGES: &[(&str, &[&str])] = &[
    ("typescript",     &["ts"]),
    ("javascript",     &["js", "node"]),
    ("rust",           &["rs"]),
    ("go",             &["golang"]),
    ("python",         &["py"]),
    ("c",              &[]),
    ("cpp",            &["c++", "cxx"]),
    ("zig",            &[]),
    ("assemblyscript", &["as"]),
    ("csharp",         &["cs", "c#", "dotnet"]),
    ("swift",          &[]),
    ("kotlin",         &["kt"]),
    ("java",           &[]),
    ("ruby",           &["rb"]),
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
        "rust" => vec![
            ("src/lib.rs",  include_str!("../../scaffolds/functions/rust/src/lib.rs")),
            ("Cargo.toml",  include_str!("../../scaffolds/functions/rust/Cargo.toml")),
            ("flux.json",   include_str!("../../scaffolds/functions/rust/flux.json")),
        ],
        "go" => vec![
            ("main.go",   include_str!("../../scaffolds/functions/go/main.go")),
            ("go.mod",    include_str!("../../scaffolds/functions/go/go.mod")),
            ("flux.json", include_str!("../../scaffolds/functions/go/flux.json")),
        ],
        "python" => vec![
            ("handler.py",       include_str!("../../scaffolds/functions/python/handler.py")),
            ("requirements.txt", include_str!("../../scaffolds/functions/python/requirements.txt")),
            ("flux.json",        include_str!("../../scaffolds/functions/python/flux.json")),
        ],
        "c" => vec![
            ("handler.c", include_str!("../../scaffolds/functions/c/handler.c")),
            ("Makefile",  include_str!("../../scaffolds/functions/c/Makefile")),
            ("flux.json", include_str!("../../scaffolds/functions/c/flux.json")),
        ],
        "cpp" => vec![
            ("handler.cpp", include_str!("../../scaffolds/functions/cpp/handler.cpp")),
            ("Makefile",    include_str!("../../scaffolds/functions/cpp/Makefile")),
            ("flux.json",   include_str!("../../scaffolds/functions/cpp/flux.json")),
        ],
        "zig" => vec![
            ("handler.zig", include_str!("../../scaffolds/functions/zig/handler.zig")),
            ("build.zig",   include_str!("../../scaffolds/functions/zig/build.zig")),
            ("flux.json",   include_str!("../../scaffolds/functions/zig/flux.json")),
        ],
        "assemblyscript" => vec![
            ("index.ts",                include_str!("../../scaffolds/functions/assemblyscript/index.ts")),
            ("flux.json",               include_str!("../../scaffolds/functions/assemblyscript/flux.json")),
            ("asconfig.json",           include_str!("../../scaffolds/functions/assemblyscript/asconfig.json")),
            ("tsconfig.json",           include_str!("../../scaffolds/functions/assemblyscript/tsconfig.json")),
            ("assembly.d.ts",           include_str!("../../scaffolds/functions/assemblyscript/assembly.d.ts")),
            ("@fluxbase-functions.ts",  include_str!("../../scaffolds/functions/assemblyscript/@fluxbase-functions.ts")),
        ],
        "csharp" => vec![
            ("Handler.cs",     include_str!("../../scaffolds/functions/csharp/Handler.cs")),
            ("Handler.csproj", include_str!("../../scaffolds/functions/csharp/Handler.csproj")),
            ("flux.json",      include_str!("../../scaffolds/functions/csharp/flux.json")),
        ],
        "swift" => vec![
            ("Handler.swift", include_str!("../../scaffolds/functions/swift/Handler.swift")),
            ("Package.swift", include_str!("../../scaffolds/functions/swift/Package.swift")),
            ("flux.json",     include_str!("../../scaffolds/functions/swift/flux.json")),
        ],
        "kotlin" => vec![
            ("Handler.kt",           include_str!("../../scaffolds/functions/kotlin/Handler.kt")),
            ("build.gradle.kts",     include_str!("../../scaffolds/functions/kotlin/build.gradle.kts")),
            ("settings.gradle.kts",  include_str!("../../scaffolds/functions/kotlin/settings.gradle.kts")),
            ("flux.json",            include_str!("../../scaffolds/functions/kotlin/flux.json")),
        ],
        "java" => vec![
            ("Handler.java",   include_str!("../../scaffolds/functions/java/Handler.java")),
            ("build.gradle",   include_str!("../../scaffolds/functions/java/build.gradle")),
            ("settings.gradle",include_str!("../../scaffolds/functions/java/settings.gradle")),
            ("flux.json",      include_str!("../../scaffolds/functions/java/flux.json")),
        ],
        "ruby" => vec![
            ("handler.rb", include_str!("../../scaffolds/functions/ruby/handler.rb")),
            ("Gemfile",    include_str!("../../scaffolds/functions/ruby/Gemfile")),
            ("flux.json",  include_str!("../../scaffolds/functions/ruby/flux.json")),
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
