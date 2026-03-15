use clap::{Subcommand, ValueEnum};
use crate::client::ApiClient;
use serde_json::Value;
use api_contract::routes as R;

// ── Language enum ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, ValueEnum)]
pub enum Language {
    /// TypeScript — runs on Deno V8
    Typescript,
    /// JavaScript — runs on Deno V8
    Javascript,
}

impl Language {
    fn as_str(&self) -> &'static str {
        match self {
            Language::Typescript => "typescript",
            Language::Javascript => "javascript",
        }
    }
}

// ── CLI commands ──────────────────────────────────────────────────────────────

#[derive(Subcommand)]
pub enum FunctionCommands {
    /// Scaffold a new serverless function (TypeScript or JavaScript, runs on Deno V8)
    ///
    /// Examples:
    ///   flux function create greet
    ///   flux function create greet --language javascript
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
        ("typescript", "deno", "Node.js (for bundling)", "https://nodejs.org"),
        ("javascript", "deno", "Node.js (for bundling)", "https://nodejs.org"),
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
