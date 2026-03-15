use clap::{Subcommand, ValueEnum};
use crate::client::ApiClient;
use serde_json::Value;
use api_contract::routes as R;

// ── Language enum ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, ValueEnum)]
pub enum Language {
    /// TypeScript — runs on Deno (no WASM compile step)
    Typescript,
    /// JavaScript — runs on Deno (no WASM compile step)
    Javascript,
    /// Rust — compiled to WASM via `cargo build --target wasm32-wasip1`
    Rust,
    /// Go — compiled to WASM via TinyGo
    Go,
    /// Python — compiled via py2wasm
    Python,
    /// C — compiled via wasi-sdk clang
    C,
    /// C++ — compiled via wasi-sdk clang++
    Cpp,
    /// Zig — compiled with `zig build-lib`
    Zig,
    /// AssemblyScript — compiled via `npx asc`
    Assemblyscript,
    /// C# — compiled via dotnet wasi
    Csharp,
    /// Swift — compiled via swiftwasm
    Swift,
    /// Kotlin — compiled via Kotlin/Wasm
    Kotlin,
    /// Java — compiled via GraalVM Native Image
    Java,
    /// Ruby — compiled via ruby.wasm
    Ruby,
}

impl Language {
    fn as_str(&self) -> &'static str {
        match self {
            Language::Typescript     => "typescript",
            Language::Javascript     => "javascript",
            Language::Rust           => "rust",
            Language::Go             => "go",
            Language::Python         => "python",
            Language::C              => "c",
            Language::Cpp            => "cpp",
            Language::Zig            => "zig",
            Language::Assemblyscript => "assemblyscript",
            Language::Csharp         => "csharp",
            Language::Swift          => "swift",
            Language::Kotlin         => "kotlin",
            Language::Java           => "java",
            Language::Ruby           => "ruby",
        }
    }
}

// ── CLI commands ──────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum FunctionCommands {
    /// Scaffold a new serverless function
    ///
    /// Defaults to TypeScript (Deno). Use --language to choose a WASM language.
    ///
    /// Examples:
    ///   flux function create greet
    ///   flux function create greet --language rust
    ///   flux function create greet --language go
    ///   flux function create greet --language assemblyscript
    ///   flux function create greet --language c
    ///   flux function create greet --language zig
    ///   flux function create greet --language python
    Create {
        name: String,
        /// Language to scaffold (default: typescript)
        #[arg(long, short, value_enum, default_value = "typescript")]
        language: Language,
    },
    /// List deployed functions in the current project
    List,
    /// Show supported languages and their required toolchains
    Languages,
}

// ── Dispatch ──────────────────────────────────────────────────────────────────

pub async fn execute(command: FunctionCommands) -> anyhow::Result<()> {
    match command {
        FunctionCommands::Create { name, language } => {
            // Delegate to new_function which scaffolds into functions/<name>/
            // with all 14 languages and proper flux.json.
            crate::new_function::execute_new_function(name, Some(language.as_str().to_owned()))?;
        }
        FunctionCommands::List => {
            list_functions().await?;
        }
        FunctionCommands::Languages => {
            print_languages();
        }
    }
    Ok(())
}

// ── Languages table ───────────────────────────────────────────────────────────

fn print_languages() {
    println!("{:<16} {:<10} {:<55} INSTALL", "LANGUAGE", "RUNTIME", "TOOLCHAIN");
    println!("{}", "-".repeat(115));
    let rows: &[(&str, &str, &str, &str)] = &[
        ("typescript",     "deno",  "Node.js (for bundling)",                               "https://nodejs.org"),
        ("javascript",     "deno",  "Node.js (for bundling)",                               "https://nodejs.org"),
        ("rust",           "wasm",  "rustup target add wasm32-wasip1",                      "https://rustup.rs"),
        ("go",             "wasm",  "TinyGo — tinygo build",                               "https://tinygo.org"),
        ("python",         "wasm",  "py2wasm -i handler.py -o handler.wasm",               "https://github.com/astral-sh/py2wasm"),
        ("c",              "wasm",  "wasi-sdk — clang --target=wasm32-wasi",               "https://github.com/WebAssembly/wasi-sdk"),
        ("cpp",            "wasm",  "wasi-sdk — clang++ --target=wasm32-wasi",             "https://github.com/WebAssembly/wasi-sdk"),
        ("zig",            "wasm",  "zig build-lib -target wasm32-freestanding",           "https://ziglang.org"),
        ("assemblyscript", "wasm",  "npx asc index.ts --target release",                   "https://assemblyscript.org"),
        ("csharp",         "wasm",  "dotnet add package Wasi.Sdk",                          "https://github.com/dotnet/dotnet-wasi-sdk"),
        ("swift",          "wasm",  "swiftc -target wasm32-unknown-wasi",                  "https://swiftwasm.org"),
        ("kotlin",         "wasm",  "Kotlin/Wasm (Gradle wasmWasiJar)",                    "https://kotl.in/wasm"),
        ("java",           "wasm",  "GraalVM native-image --no-fallback",                  "https://graalvm.org"),
        ("ruby",           "wasm",  "ruby.wasm build handler.rb",                          "https://ruby.wasm"),
    ];
    for (lang, rt, toolchain, url) in rows {
        println!("{:<16} {:<10} {:<55} {}", lang, rt, toolchain, url);
    }
}

// ── List deployed functions ───────────────────────────────────────────────────

async fn list_functions() -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let res = client.client
        .get(R::functions::LIST.url(&client.base_url))
        .send()
        .await?;
    let json: Value = res.error_for_status()?.json().await?;
    let functions = json
        .get("data")
        .and_then(|d| d.get("functions"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    println!("{:<40} {:<25} {:<10} DESCRIPTION", "ID", "NAME", "RUNTIME");
    println!("{}", "-".repeat(100));
    for func in functions {
        let id      = func.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let name    = func.get("name").and_then(|v| v.as_str()).unwrap_or("");
        let runtime = func.get("runtime").and_then(|v| v.as_str()).unwrap_or("");
        let desc    = func.get("description").and_then(|v| v.as_str()).unwrap_or("-");
        println!("{:<40} {:<25} {:<10} {}", id, name, runtime, desc);
    }
    Ok(())
}
