//! `flux generate` — fetch the live project manifest and write typed ctx
//! bindings for all 14 target languages.
//!
//! # Single Responsibility
//! Given a [`Manifest`] (fetched from `GET /sdk/manifest`), write one file per
//! language into the `.flux/` output directory.  No other concerns live here.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use colored::Colorize;
use serde::Deserialize;

use crate::client::ApiClient;

// ── Manifest types ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub project_id:   String,
    pub generated_at: String,
    pub schema_hash:  String,
    pub database:     BTreeMap<String, TableSchema>,
    pub functions:    BTreeMap<String, FunctionSchema>,
    pub secrets:      Vec<String>,
    pub agents:       BTreeMap<String, AgentSchema>,
}

#[derive(Debug, Deserialize)]
pub struct TableSchema {
    pub columns: Vec<ColumnDef>,
}

#[derive(Debug, Deserialize)]
pub struct ColumnDef {
    pub name:     String,
    #[serde(rename = "type")]
    pub col_type: String,
    pub nullable: bool,
}

#[derive(Debug, Deserialize)]
pub struct FunctionSchema {
    pub input_schema:  Option<serde_json::Value>,
    pub output_schema: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct AgentSchema {
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn execute_generate(output_dir: Option<String>) -> anyhow::Result<()> {
    let flux_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(".flux"));

    println!("{} Fetching manifest\u{2026}", "◆".blue());
    let client = ApiClient::new().await?;
    let (manifest, raw) = fetch_manifest(&client).await?;

    tokio::fs::create_dir_all(&flux_dir).await?;
    let manifest_path = flux_dir.join("manifest.json");
    tokio::fs::write(&manifest_path, serde_json::to_string_pretty(&raw)?).await?;
    println!(
        "{} manifest.json written  (hash: {})",
        "\u{2714}".green().bold(),
        manifest.schema_hash
    );
    println!("  project  {}  \u{00b7}  {}", manifest.project_id, manifest.generated_at);
    println!();

    let results = generate_all(&manifest, &flux_dir);

    let col1 = 14usize;
    let col2 = 30usize;
    println!("{:<col1$}  {:<col2$}  {}", "Language", "File", "Status");
    println!("{}", "\u{2500}".repeat(col1 + col2 + 14));

    for (lang, file, result) in &results {
        match result {
            Ok(path) => println!(
                "{:<col1$}  {:<col2$}  {}",
                lang,
                path.display(),
                "\u{2714} ok".green()
            ),
            Err(e) => println!(
                "{:<col1$}  {:<col2$}  {} {}",
                lang,
                file,
                "\u{2716}".red(),
                e
            ),
        }
    }

    let ok = results.iter().filter(|(_, _, r)| r.is_ok()).count();
    let total = results.len();
    println!();
    println!(
        "{} Generated {}/{} language bindings in {}",
        "\u{25c6}".blue(),
        ok,
        total,
        flux_dir.display()
    );

    Ok(())
}

// ── Manifest fetch ────────────────────────────────────────────────────────────

async fn fetch_manifest(client: &ApiClient) -> anyhow::Result<(Manifest, serde_json::Value)> {
    let url = format!("{}/sdk/manifest", client.base_url);
    let resp = client.client.get(&url).send().await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("server returned {}: {}", status, body);
    }

    let raw: serde_json::Value = resp.json().await?;
    let manifest: Manifest = serde_json::from_value(raw.clone())
        .map_err(|e| anyhow::anyhow!("failed to parse manifest: {}", e))?;

    Ok((manifest, raw))
}

// ── Dispatch all generators ───────────────────────────────────────────────────

fn generate_all(
    manifest: &Manifest,
    out_dir: &Path,
) -> Vec<(&'static str, &'static str, anyhow::Result<PathBuf>)> {
    vec![
        ("TypeScript",     "types.d.ts",  generate_typescript(manifest, out_dir)),
        ("JavaScript",     "ctx.js",       generate_javascript(manifest, out_dir)),
        ("Rust",           "ctx.rs",       generate_rust(manifest, out_dir)),
        ("Go",             "ctx.go",       generate_go(manifest, out_dir)),
        ("Python",         "ctx.pyi",      generate_python(manifest, out_dir)),
        ("C",              "ctx.h",        generate_c(manifest, out_dir)),
        ("C++",            "ctx.hpp",      generate_cpp(manifest, out_dir)),
        ("Zig",            "ctx.zig",      generate_zig(manifest, out_dir)),
        ("AssemblyScript", "ctx.as.ts",    generate_assemblyscript(manifest, out_dir)),
        ("C#",             "Ctx.cs",       generate_csharp(manifest, out_dir)),
        ("Swift",          "Ctx.swift",    generate_swift(manifest, out_dir)),
        ("Kotlin",         "Ctx.kt",       generate_kotlin(manifest, out_dir)),
        ("Java",           "Ctx.java",     generate_java(manifest, out_dir)),
        ("Ruby",           "ctx.rb",       generate_ruby(manifest, out_dir)),
    ]
}

// ── Type-mapping helpers ──────────────────────────────────────────────────────

fn pg_to_ts(pg: &str) -> &'static str {
    match pg {
        "uuid" | "character varying" | "text" | "varchar" | "char" | "bpchar" => "string",
        "integer" | "int" | "int4" | "int2" | "smallint" | "bigint" | "int8" => "number",
        "boolean" | "bool" => "boolean",
        "numeric" | "decimal" | "float4" | "float8" | "real" | "double precision" => "number",
        "json" | "jsonb" => "unknown",
        _ => "unknown",
    }
}

fn pg_to_rust(pg: &str) -> &'static str {
    match pg {
        "uuid" => "uuid::Uuid",
        "text" | "character varying" | "varchar" | "char" | "bpchar" => "String",
        "integer" | "int4" | "int" => "i32",
        "smallint" | "int2" => "i16",
        "bigint" | "int8" => "i64",
        "boolean" | "bool" => "bool",
        "numeric" | "decimal" | "float8" | "double precision" => "f64",
        "float4" | "real" => "f32",
        "json" | "jsonb" => "serde_json::Value",
        "timestamp with time zone" | "timestamptz" => "chrono::DateTime<chrono::Utc>",
        "timestamp" | "timestamp without time zone" => "chrono::NaiveDateTime",
        "date" => "chrono::NaiveDate",
        _ => "serde_json::Value",
    }
}

fn pg_to_go(pg: &str) -> &'static str {
    match pg {
        "uuid" | "text" | "character varying" | "varchar" | "char" | "bpchar" => "string",
        "integer" | "int4" | "int" | "smallint" | "int2" => "int32",
        "bigint" | "int8" => "int64",
        "boolean" | "bool" => "bool",
        "numeric" | "decimal" | "float8" | "double precision" => "float64",
        "float4" | "real" => "float32",
        "json" | "jsonb" => "interface{}",
        "timestamp with time zone" | "timestamptz" | "timestamp" | "timestamp without time zone" => "time.Time",
        _ => "interface{}",
    }
}

fn pg_to_python(pg: &str) -> &'static str {
    match pg {
        "uuid" | "text" | "character varying" | "varchar" | "char" | "bpchar" => "str",
        "integer" | "int4" | "int" | "smallint" | "int2" | "bigint" | "int8" => "int",
        "boolean" | "bool" => "bool",
        "numeric" | "decimal" | "float8" | "double precision" | "float4" | "real" => "float",
        "json" | "jsonb" => "Any",
        "timestamp with time zone" | "timestamptz" | "timestamp" | "timestamp without time zone" => "datetime",
        _ => "Any",
    }
}

fn pg_to_c(pg: &str) -> &'static str {
    match pg {
        "uuid" | "text" | "character varying" | "varchar" | "char" | "bpchar"
        | "json" | "jsonb" => "const char*",
        "integer" | "int4" | "int" => "int32_t",
        "smallint" | "int2" => "int16_t",
        "bigint" | "int8" => "int64_t",
        "boolean" | "bool" => "bool",
        "numeric" | "decimal" | "float8" | "double precision" => "double",
        "float4" | "real" => "float",
        "timestamp with time zone" | "timestamptz" | "timestamp"
        | "timestamp without time zone" => "const char*",
        _ => "const char*",
    }
}

fn pg_to_csharp(pg: &str) -> &'static str {
    match pg {
        "uuid" => "Guid",
        "text" | "character varying" | "varchar" | "char" | "bpchar" => "string",
        "integer" | "int4" | "int" | "smallint" | "int2" => "int",
        "bigint" | "int8" => "long",
        "boolean" | "bool" => "bool",
        "numeric" | "decimal" | "float8" | "double precision" => "double",
        "float4" | "real" => "float",
        "json" | "jsonb" => "System.Text.Json.JsonElement",
        "timestamp with time zone" | "timestamptz" | "timestamp"
        | "timestamp without time zone" => "DateTime",
        _ => "object",
    }
}

fn pg_to_swift(pg: &str) -> &'static str {
    match pg {
        "uuid" => "UUID",
        "text" | "character varying" | "varchar" | "char" | "bpchar" => "String",
        "integer" | "int4" | "int" | "smallint" | "int2" => "Int32",
        "bigint" | "int8" => "Int64",
        "boolean" | "bool" => "Bool",
        "numeric" | "decimal" | "float8" | "double precision" => "Double",
        "float4" | "real" => "Float",
        "json" | "jsonb" => "[String: AnyCodable]",
        "timestamp with time zone" | "timestamptz" | "timestamp"
        | "timestamp without time zone" => "Date",
        _ => "AnyCodable",
    }
}

fn pg_to_kotlin(pg: &str) -> &'static str {
    match pg {
        "uuid" => "java.util.UUID",
        "text" | "character varying" | "varchar" | "char" | "bpchar" => "String",
        "integer" | "int4" | "int" | "smallint" | "int2" => "Int",
        "bigint" | "int8" => "Long",
        "boolean" | "bool" => "Boolean",
        "numeric" | "decimal" | "float8" | "double precision" | "float4" | "real" => "Double",
        "json" | "jsonb" => "com.google.gson.JsonObject",
        "timestamp with time zone" | "timestamptz" | "timestamp"
        | "timestamp without time zone" => "java.time.Instant",
        _ => "Any",
    }
}

fn pg_to_java(pg: &str) -> &'static str {
    match pg {
        "uuid" => "java.util.UUID",
        "text" | "character varying" | "varchar" | "char" | "bpchar" => "String",
        "integer" | "int4" | "int" | "smallint" | "int2" => "int",
        "bigint" | "int8" => "long",
        "boolean" | "bool" => "boolean",
        "numeric" | "decimal" | "float8" | "double precision" | "float4" | "real" => "double",
        "json" | "jsonb" => "jakarta.json.JsonObject",
        "timestamp with time zone" | "timestamptz" | "timestamp"
        | "timestamp without time zone" => "java.time.Instant",
        _ => "Object",
    }
}

fn pg_to_zig(pg: &str) -> &'static str {
    match pg {
        "uuid" | "text" | "character varying" | "varchar" | "char" | "bpchar"
        | "json" | "jsonb" => "[]const u8",
        "integer" | "int4" | "int" => "i32",
        "smallint" | "int2" => "i16",
        "bigint" | "int8" => "i64",
        "boolean" | "bool" => "bool",
        "numeric" | "decimal" | "float8" | "double precision" => "f64",
        "float4" | "real" => "f32",
        "timestamp with time zone" | "timestamptz" | "timestamp"
        | "timestamp without time zone" => "[]const u8",
        _ => "[]const u8",
    }
}

fn pg_to_as(pg: &str) -> &'static str {
    match pg {
        "uuid" | "text" | "character varying" | "varchar" | "char" | "bpchar"
        | "json" | "jsonb" => "string",
        "integer" | "int4" | "int" | "smallint" | "int2" => "i32",
        "bigint" | "int8" => "i64",
        "boolean" | "bool" => "bool",
        "numeric" | "decimal" | "float8" | "double precision" => "f64",
        "float4" | "real" => "f32",
        "timestamp with time zone" | "timestamptz" | "timestamp"
        | "timestamp without time zone" => "string",
        _ => "string",
    }
}

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None    => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect()
}

fn json_schema_prop_to_ts(prop: &serde_json::Value) -> &'static str {
    match prop.get("type").and_then(|v| v.as_str()) {
        Some("string")  => "string",
        Some("number")  => "number",
        Some("integer") => "number",
        Some("boolean") => "boolean",
        Some("array")   => "unknown[]",
        Some("object")  => "Record<string, unknown>",
        _               => "unknown",
    }
}

fn json_schema_to_ts_fields(schema: &serde_json::Value) -> String {
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return String::new();
    };
    props
        .iter()
        .map(|(key, val)| format!("  {}: {};", key, json_schema_prop_to_ts(val)))
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Generator 1: TypeScript ───────────────────────────────────────────────────

fn generate_typescript(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("// Generated by flux generate \u{2014} do not edit\n");
    out.push_str("// Regenerate: flux generate\n\n");

    // DB row interfaces
    out.push_str("// \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    for (table, schema) in &manifest.database {
        out.push_str(&format!("export interface {} {{\n", to_pascal_case(table)));
        for col in &schema.columns {
            let ts_type = if col.nullable {
                format!("{} | null", pg_to_ts(&col.col_type))
            } else {
                pg_to_ts(&col.col_type).to_string()
            };
            out.push_str(&format!("  {}: {};\n", col.name, ts_type));
        }
        out.push_str("}\n\n");
    }

    // FluxDbTable + FluxDb
    out.push_str("// \u{2500}\u{2500} Database client \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    out.push_str("export interface FluxDbTable<T> {\n");
    out.push_str("  find(where?: Partial<T>): Promise<T[]>;\n");
    out.push_str("  findOne(where?: Partial<T>): Promise<T | null>;\n");
    out.push_str("  insert(data: Omit<T, 'id'>): Promise<T>;\n");
    out.push_str("  update(where: Partial<T>, data: Partial<T>): Promise<T[]>;\n");
    out.push_str("  delete(where: Partial<T>): Promise<void>;\n");
    out.push_str("}\n\n");
    out.push_str("export interface FluxDb {\n");
    for table in manifest.database.keys() {
        out.push_str(&format!("  {}: FluxDbTable<{}>;\n", table, to_pascal_case(table)));
    }
    out.push_str("}\n\n");

    // Functions
    out.push_str("// \u{2500}\u{2500} Functions \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    for (name, fn_schema) in &manifest.functions {
        let pascal = to_pascal_case(name);
        let input_fields = fn_schema.input_schema.as_ref()
            .map(|s| json_schema_to_ts_fields(s))
            .unwrap_or_default();
        if input_fields.is_empty() {
            out.push_str(&format!("export type {}Input = unknown;\n", pascal));
        } else {
            out.push_str(&format!("export interface {}Input {{\n{}\n}}\n", pascal, input_fields));
        }
        let output_fields = fn_schema.output_schema.as_ref()
            .map(|s| json_schema_to_ts_fields(s))
            .unwrap_or_default();
        if output_fields.is_empty() {
            out.push_str(&format!("export type {}Output = unknown;\n", pascal));
        } else {
            out.push_str(&format!("export interface {}Output {{\n{}\n}}\n", pascal, output_fields));
        }
        out.push('\n');
    }
    out.push_str("export interface FluxFunctions {\n");
    for name in manifest.functions.keys() {
        let pascal = to_pascal_case(name);
        out.push_str(&format!("  {}(input: {}Input): Promise<{}Output>;\n", name, pascal, pascal));
    }
    out.push_str("}\n\n");

    // Secrets
    out.push_str("// \u{2500}\u{2500} Secrets \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    if manifest.secrets.is_empty() {
        out.push_str("export type FluxSecretKey = string;\n");
    } else {
        let union = manifest.secrets.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(" | ");
        out.push_str(&format!("export type FluxSecretKey = {};\n", union));
    }
    out.push_str("export type FluxSecrets = { readonly [K in FluxSecretKey]: string };\n\n");

    // Agents
    out.push_str("// \u{2500}\u{2500} Agents \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    out.push_str("export interface FluxAgents {\n");
    for name in manifest.agents.keys() {
        out.push_str(&format!("  {}: {{ run(goal: string): Promise<unknown> }};\n", name));
    }
    out.push_str("}\n\n");

    // FluxContext
    out.push_str("// \u{2500}\u{2500} Context \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    out.push_str("export interface FluxContext {\n");
    out.push_str("  db:        FluxDb;\n");
    out.push_str("  functions: FluxFunctions;\n");
    out.push_str("  secrets:   FluxSecrets;\n");
    out.push_str("  agents:    FluxAgents;\n");
    out.push_str("  log(message: string, level?: 'info' | 'warn' | 'error'): void;\n");
    out.push_str("}\n");

    let path = out_dir.join("types.d.ts");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 2: JavaScript (JSDoc) ──────────────────────────────────────────

fn generate_javascript(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("// Generated by flux generate \u{2014} do not edit\n");
    out.push_str("// @ts-check\n\n");

    for (table, schema) in &manifest.database {
        out.push_str(&format!("/**\n * @typedef {{Object}} {}\n", to_pascal_case(table)));
        for col in &schema.columns {
            out.push_str(&format!(" * @property {{{}}} {}\n", pg_to_ts(&col.col_type), col.name));
        }
        out.push_str(" */\n\n");
    }

    out.push_str("/**\n * @typedef {Object} FluxDb\n");
    for table in manifest.database.keys() {
        out.push_str(&format!(" * @property {{FluxDbTable<{}>}} {}\n", to_pascal_case(table), table));
    }
    out.push_str(" */\n\n");

    out.push_str("/**\n");
    out.push_str(" * @template T\n");
    out.push_str(" * @typedef {Object} FluxDbTable\n");
    out.push_str(" * @property {function(Partial<T>=): Promise<T[]>} find\n");
    out.push_str(" * @property {function(Partial<T>=): Promise<T|null>} findOne\n");
    out.push_str(" * @property {function(T): Promise<T>} insert\n");
    out.push_str(" * @property {function(Partial<T>, Partial<T>): Promise<T[]>} update\n");
    out.push_str(" * @property {function(Partial<T>): Promise<void>} delete\n");
    out.push_str(" */\n\n");

    if !manifest.secrets.is_empty() {
        let union = manifest.secrets.iter().map(|s| format!("\"{}\"", s)).collect::<Vec<_>>().join(" | ");
        out.push_str(&format!("/**\n * @typedef {{{}}} FluxSecretKey\n */\n\n", union));
    }

    out.push_str("/**\n");
    out.push_str(" * @typedef {Object} FluxContext\n");
    out.push_str(" * @property {FluxDb} db\n");
    out.push_str(" * @property {Object} functions\n");
    out.push_str(" * @property {Object} secrets\n");
    out.push_str(" * @property {Object} agents\n");
    out.push_str(" * @property {function(string, string=): void} log\n");
    out.push_str(" */\n\n");
    out.push_str("module.exports = {};\n");

    let path = out_dir.join("ctx.js");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 3: Rust ─────────────────────────────────────────────────────────

fn generate_rust(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("//! Generated by flux generate \u{2014} do not edit\n");
    out.push_str("//! Regenerate: flux generate\n\n");
    out.push_str("use serde::{Deserialize, Serialize};\n\n");
    out.push_str("// \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");

    for (table, schema) in &manifest.database {
        out.push_str("#[derive(Debug, Clone, Serialize, Deserialize)]\n");
        out.push_str(&format!("pub struct {} {{\n", to_pascal_case(table)));
        for col in &schema.columns {
            let rust_type = pg_to_rust(&col.col_type);
            if col.nullable {
                out.push_str(&format!("    pub {}: Option<{}>,\n", col.name, rust_type));
            } else {
                out.push_str(&format!("    pub {}: {},\n", col.name, rust_type));
            }
        }
        out.push_str("}\n\n");
    }

    out.push_str("// \u{2500}\u{2500} Ctx type stubs \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n");
    out.push_str("// The actual runtime implementation lives in the Flux SDK crate.\n\n");
    out.push_str("pub trait FluxDbTable<T> {\n");
    out.push_str("    fn find(&self, filter: serde_json::Value) -> Vec<T>;\n");
    out.push_str("    fn find_one(&self, filter: serde_json::Value) -> Option<T>;\n");
    out.push_str("    fn insert(&self, data: T) -> T;\n");
    out.push_str("    fn update(&self, filter: serde_json::Value, data: T) -> Vec<T>;\n");
    out.push_str("    fn delete(&self, filter: serde_json::Value);\n");
    out.push_str("}\n\n");
    out.push_str("pub struct FluxDb;\n");
    out.push_str("impl FluxDb {\n");
    for table in manifest.database.keys() {
        out.push_str(&format!(
            "    pub fn {}(&self) -> &dyn FluxDbTable<{}> {{ unimplemented!(\"provided by runtime\") }}\n",
            table, to_pascal_case(table)
        ));
    }
    out.push_str("}\n\n");

    if !manifest.secrets.is_empty() {
        out.push_str("/// Secret key constants \u{2014} use with `ctx.secrets.get(secret_keys::KEY)`.\n");
        out.push_str("pub mod secret_keys {\n");
        for key in &manifest.secrets {
            let const_name = key.to_uppercase().replace('-', "_");
            out.push_str(&format!("    pub const {}: &str = \"{}\";\n", const_name, key));
        }
        out.push_str("}\n\n");
    }

    out.push_str("pub struct FluxContext {\n");
    out.push_str("    pub db: FluxDb,\n");
    out.push_str("}\n");

    let path = out_dir.join("ctx.rs");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 4: Go ───────────────────────────────────────────────────────────

fn generate_go(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let needs_time = manifest.database.values().any(|s| {
        s.columns.iter().any(|c| pg_to_go(&c.col_type) == "time.Time")
    });

    let mut out = String::new();
    out.push_str("// Code generated by flux generate \u{2014} do not edit.\n");
    out.push_str("// Regenerate: flux generate\n\n");
    out.push_str("package fluxctx\n\n");
    if needs_time {
        out.push_str("import \"time\"\n\n");
    }

    out.push_str("// \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    for (table, schema) in &manifest.database {
        out.push_str(&format!("type {} struct {{\n", to_pascal_case(table)));
        for col in &schema.columns {
            let go_type = pg_to_go(&col.col_type);
            let field   = to_pascal_case(&col.name);
            let actual  = if col.nullable { format!("*{}", go_type) } else { go_type.to_string() };
            out.push_str(&format!("\t{} {} `json:\"{}\"`\n", field, actual, col.name));
        }
        out.push_str("}\n\n");
    }

    if !manifest.secrets.is_empty() {
        out.push_str("// \u{2500}\u{2500} Secret key constants \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
        out.push_str("const (\n");
        for key in &manifest.secrets {
            let const_name = format!(
                "Secret{}",
                to_pascal_case(&key.to_lowercase().replace('-', "_"))
            );
            out.push_str(&format!("\t{} = \"{}\"\n", const_name, key));
        }
        out.push_str(")\n\n");
    }

    out.push_str("// \u{2500}\u{2500} Context \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    out.push_str("type FluxDb struct{}\n");
    out.push_str("type FluxFunctions struct{}\n");
    out.push_str("type FluxSecrets struct{}\n");
    out.push_str("type FluxAgents struct{}\n\n");
    out.push_str("type FluxContext struct {\n");
    out.push_str("\tDB        FluxDb\n");
    out.push_str("\tFunctions FluxFunctions\n");
    out.push_str("\tSecrets   FluxSecrets\n");
    out.push_str("\tAgents    FluxAgents\n");
    out.push_str("}\n");

    let path = out_dir.join("ctx.go");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 5: Python stub ──────────────────────────────────────────────────

fn generate_python(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let needs_datetime = manifest.database.values().any(|s| {
        s.columns.iter().any(|c| pg_to_python(&c.col_type) == "datetime")
    });

    let mut out = String::new();
    out.push_str("# Generated by flux generate \u{2014} do not edit\n");
    out.push_str("# Regenerate: flux generate\n\n");
    out.push_str("from __future__ import annotations\n");
    out.push_str("from typing import Any, Optional, List\n");
    if needs_datetime {
        out.push_str("from datetime import datetime\n");
    }
    out.push('\n');

    out.push_str("# \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    for (table, schema) in &manifest.database {
        out.push_str(&format!("class {}:\n", to_pascal_case(table)));
        if schema.columns.is_empty() {
            out.push_str("    pass\n\n");
        } else {
            for col in &schema.columns {
                let py_type = pg_to_python(&col.col_type);
                if col.nullable {
                    out.push_str(&format!("    {}: Optional[{}]\n", col.name, py_type));
                } else {
                    out.push_str(&format!("    {}: {}\n", col.name, py_type));
                }
            }
            out.push('\n');
        }
    }

    if !manifest.secrets.is_empty() {
        out.push_str("# \u{2500}\u{2500} Secret key constants \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
        for key in &manifest.secrets {
            out.push_str(&format!("{} = \"{}\"\n", key.to_uppercase().replace('-', "_"), key));
        }
        out.push('\n');
    }

    out.push_str("# \u{2500}\u{2500} Context \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    out.push_str("class FluxDb: ...\n");
    out.push_str("class FluxFunctions: ...\n");
    out.push_str("class FluxSecrets: ...\n");
    out.push_str("class FluxAgents: ...\n\n");
    out.push_str("class FluxContext:\n");
    out.push_str("    db: FluxDb\n");
    out.push_str("    functions: FluxFunctions\n");
    out.push_str("    secrets: FluxSecrets\n");
    out.push_str("    agents: FluxAgents\n");
    out.push_str("    def log(self, message: str, level: str = \"info\") -> None: ...\n");

    let path = out_dir.join("ctx.pyi");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 6: C header ─────────────────────────────────────────────────────

fn generate_c(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("/* Generated by flux generate \u{2014} do not edit */\n");
    out.push_str("/* Regenerate: flux generate */\n");
    out.push_str("#ifndef FLUX_CTX_H\n");
    out.push_str("#define FLUX_CTX_H\n\n");
    out.push_str("#include <stdint.h>\n");
    out.push_str("#include <stdbool.h>\n\n");
    out.push_str("/* \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500} */\n\n");

    for (table, schema) in &manifest.database {
        out.push_str("typedef struct {\n");
        for col in &schema.columns {
            out.push_str(&format!("    {} {};\n", pg_to_c(&col.col_type), col.name));
        }
        out.push_str(&format!("}} {};\n\n", to_pascal_case(table)));
    }

    if !manifest.secrets.is_empty() {
        out.push_str("/* \u{2500}\u{2500} Secret key constants \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500} */\n\n");
        for key in &manifest.secrets {
            let macro_name = format!("FLUX_SECRET_{}", key.to_uppercase().replace('-', "_"));
            out.push_str(&format!("#define {} \"{}\"\n", macro_name, key));
        }
        out.push('\n');
    }

    out.push_str("#endif /* FLUX_CTX_H */\n");

    let path = out_dir.join("ctx.h");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 7: C++ header ───────────────────────────────────────────────────

fn pg_to_cpp(pg: &str) -> &'static str {
    match pg {
        "uuid" | "text" | "character varying" | "varchar" | "char" | "bpchar"
        | "json" | "jsonb" => "std::string",
        "integer" | "int4" | "int" => "int32_t",
        "smallint" | "int2" => "int16_t",
        "bigint" | "int8" => "int64_t",
        "boolean" | "bool" => "bool",
        "numeric" | "decimal" | "float8" | "double precision" => "double",
        "float4" | "real" => "float",
        "timestamp with time zone" | "timestamptz" | "timestamp"
        | "timestamp without time zone" => "std::string",
        _ => "std::string",
    }
}

fn generate_cpp(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("// Generated by flux generate \u{2014} do not edit\n");
    out.push_str("// Regenerate: flux generate\n");
    out.push_str("#pragma once\n\n");
    out.push_str("#include <cstdint>\n");
    out.push_str("#include <string>\n");
    out.push_str("#include <optional>\n");
    out.push_str("#include <vector>\n\n");
    out.push_str("// \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");

    for (table, schema) in &manifest.database {
        out.push_str(&format!("struct {} {{\n", to_pascal_case(table)));
        for col in &schema.columns {
            let cpp_type = pg_to_cpp(&col.col_type);
            if col.nullable {
                out.push_str(&format!("    std::optional<{}> {};\n", cpp_type, col.name));
            } else {
                out.push_str(&format!("    {} {};\n", cpp_type, col.name));
            }
        }
        out.push_str("};\n\n");
    }

    if !manifest.secrets.is_empty() {
        out.push_str("// \u{2500}\u{2500} Secret key constants \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
        for key in &manifest.secrets {
            let const_name = format!("FLUX_SECRET_{}", key.to_uppercase().replace('-', "_"));
            out.push_str(&format!("inline constexpr const char* {} = \"{}\";\n", const_name, key));
        }
        out.push('\n');
    }

    let path = out_dir.join("ctx.hpp");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 8: Zig ──────────────────────────────────────────────────────────

fn generate_zig(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("// Generated by flux generate \u{2014} do not edit\n");
    out.push_str("// Regenerate: flux generate\n\n");
    out.push_str("// \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");

    for (table, schema) in &manifest.database {
        out.push_str(&format!("pub const {} = struct {{\n", to_pascal_case(table)));
        for col in &schema.columns {
            let zig_type = pg_to_zig(&col.col_type);
            if col.nullable {
                out.push_str(&format!("    {}: ?{},\n", col.name, zig_type));
            } else {
                out.push_str(&format!("    {}: {},\n", col.name, zig_type));
            }
        }
        out.push_str("};\n\n");
    }

    if !manifest.secrets.is_empty() {
        out.push_str("// \u{2500}\u{2500} Secret key constants \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
        out.push_str("pub const secrets = struct {\n");
        for key in &manifest.secrets {
            let const_name = key.to_uppercase().replace('-', "_");
            out.push_str(&format!("    pub const {}: []const u8 = \"{}\";\n", const_name, key));
        }
        out.push_str("};\n");
    }

    let path = out_dir.join("ctx.zig");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 9: AssemblyScript ───────────────────────────────────────────────

fn generate_assemblyscript(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("// Generated by flux generate \u{2014} do not edit\n");
    out.push_str("// Regenerate: flux generate\n");
    out.push_str("// AssemblyScript types \u{2014} use i32/i64/f32/f64 primitives (not number)\n\n");
    out.push_str("// \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");

    for (table, schema) in &manifest.database {
        out.push_str("@json\n");
        out.push_str(&format!("class {} {{\n", to_pascal_case(table)));
        for col in &schema.columns {
            let as_type = pg_to_as(&col.col_type);
            let default_val = match as_type {
                "string"       => " = \"\"",
                "i32" | "i64"  => " = 0",
                "f32" | "f64"  => " = 0.0",
                "bool"         => " = false",
                _              => " = \"\"",
            };
            out.push_str(&format!("  {}: {}{};\n", col.name, as_type, default_val));
        }
        out.push_str("}\n\n");
    }

    if !manifest.secrets.is_empty() {
        out.push_str("// \u{2500}\u{2500} Secret key constants \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
        for key in &manifest.secrets {
            let const_name = format!("FLUX_SECRET_{}", key.to_uppercase().replace('-', "_"));
            out.push_str(&format!("const {}: string = \"{}\";\n", const_name, key));
        }
    }

    let path = out_dir.join("ctx.as.ts");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 10: C# ──────────────────────────────────────────────────────────

fn generate_csharp(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("// Generated by flux generate \u{2014} do not edit\n");
    out.push_str("// Regenerate: flux generate\n");
    out.push_str("using System;\n");
    out.push_str("using System.Text.Json;\n\n");
    out.push_str("namespace Flux.Generated\n{\n");
    out.push_str("    // \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");

    for (table, schema) in &manifest.database {
        let params: Vec<String> = schema.columns.iter().map(|col| {
            let cs_type = pg_to_csharp(&col.col_type);
            let field   = to_pascal_case(&col.name);
            if col.nullable {
                format!("        {}? {}", cs_type, field)
            } else {
                format!("        {} {}", cs_type, field)
            }
        }).collect();
        out.push_str(&format!(
            "    public record {}(\n{}\n    );\n\n",
            to_pascal_case(table),
            params.join(",\n")
        ));
    }

    if !manifest.secrets.is_empty() {
        out.push_str("    // \u{2500}\u{2500} Secret key constants \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
        out.push_str("    public static class SecretKeys\n    {\n");
        for key in &manifest.secrets {
            let const_name = to_pascal_case(&key.to_lowercase().replace('-', "_"));
            out.push_str(&format!("        public const string {} = \"{}\";\n", const_name, key));
        }
        out.push_str("    }\n\n");
    }

    out.push_str("    // \u{2500}\u{2500} Context interface \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    out.push_str("    public interface IFluxContext\n    {\n");
    out.push_str("        IFluxDb Db { get; }\n");
    out.push_str("        IFluxFunctions Functions { get; }\n");
    out.push_str("        IFluxSecrets Secrets { get; }\n");
    out.push_str("    }\n");
    out.push_str("    public interface IFluxDb {}\n");
    out.push_str("    public interface IFluxFunctions {}\n");
    out.push_str("    public interface IFluxSecrets {}\n");
    out.push_str("}\n");

    let path = out_dir.join("Ctx.cs");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 11: Swift ───────────────────────────────────────────────────────

fn generate_swift(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("// Generated by flux generate \u{2014} do not edit\n");
    out.push_str("// Regenerate: flux generate\n");
    out.push_str("import Foundation\n\n");
    out.push_str("// \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");

    for (table, schema) in &manifest.database {
        out.push_str(&format!("struct {}: Codable {{\n", to_pascal_case(table)));
        for col in &schema.columns {
            let swift_type = pg_to_swift(&col.col_type);
            if col.nullable {
                out.push_str(&format!("    let {}: {}?\n", col.name, swift_type));
            } else {
                out.push_str(&format!("    let {}: {}\n", col.name, swift_type));
            }
        }
        out.push_str("}\n\n");
    }

    if !manifest.secrets.is_empty() {
        out.push_str("// \u{2500}\u{2500} Secret key constants \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
        out.push_str("enum FluxSecretKey: String {\n");
        for key in &manifest.secrets {
            // camelCase enum case from UPPER_SNAKE_KEY
            let case_name = {
                let parts: Vec<&str> = key.split('_').collect();
                let mut s = String::new();
                for (i, part) in parts.iter().enumerate() {
                    let lower = part.to_lowercase();
                    if i == 0 {
                        s.push_str(&lower);
                    } else {
                        let mut c = lower.chars();
                        match c.next() {
                            None    => {}
                            Some(f) => { s.push_str(&f.to_uppercase().collect::<String>()); s.push_str(c.as_str()); }
                        }
                    }
                }
                s
            };
            out.push_str(&format!("    case {} = \"{}\"\n", case_name, key));
        }
        out.push_str("}\n\n");
    }

    out.push_str("// \u{2500}\u{2500} Context protocol \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    out.push_str("protocol FluxDb {}\n");
    out.push_str("protocol FluxFunctions {}\n");
    out.push_str("protocol FluxSecrets {}\n");
    out.push_str("protocol FluxAgents {}\n\n");
    out.push_str("protocol FluxContext {\n");
    out.push_str("    var db: FluxDb { get }\n");
    out.push_str("    var functions: FluxFunctions { get }\n");
    out.push_str("    var secrets: FluxSecrets { get }\n");
    out.push_str("    var agents: FluxAgents { get }\n");
    out.push_str("}\n");

    let path = out_dir.join("Ctx.swift");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 12: Kotlin ──────────────────────────────────────────────────────

fn generate_kotlin(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("// Generated by flux generate \u{2014} do not edit\n");
    out.push_str("// Regenerate: flux generate\n");
    out.push_str("package flux.generated\n\n");
    out.push_str("import java.util.UUID\n");
    out.push_str("import java.time.Instant\n\n");
    out.push_str("// \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");

    for (table, schema) in &manifest.database {
        out.push_str(&format!("data class {}(\n", to_pascal_case(table)));
        let fields: Vec<String> = schema.columns.iter().map(|col| {
            let kt_type = pg_to_kotlin(&col.col_type);
            if col.nullable {
                format!("    val {}: {}?", col.name, kt_type)
            } else {
                format!("    val {}: {}", col.name, kt_type)
            }
        }).collect();
        out.push_str(&fields.join(",\n"));
        out.push_str(",\n)\n\n");
    }

    if !manifest.secrets.is_empty() {
        out.push_str("// \u{2500}\u{2500} Secret key constants \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
        out.push_str("object SecretKeys {\n");
        for key in &manifest.secrets {
            let const_name = key.to_uppercase().replace('-', "_");
            out.push_str(&format!("    const val {} = \"{}\"\n", const_name, key));
        }
        out.push_str("}\n\n");
    }

    out.push_str("// \u{2500}\u{2500} Context interface \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    out.push_str("interface FluxDb\n");
    out.push_str("interface FluxFunctions\n");
    out.push_str("interface FluxSecrets\n");
    out.push_str("interface FluxAgents\n\n");
    out.push_str("interface FluxContext {\n");
    out.push_str("    val db: FluxDb\n");
    out.push_str("    val functions: FluxFunctions\n");
    out.push_str("    val secrets: FluxSecrets\n");
    out.push_str("    val agents: FluxAgents\n");
    out.push_str("}\n");

    let path = out_dir.join("Ctx.kt");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 13: Java ────────────────────────────────────────────────────────

fn generate_java(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("// Generated by flux generate \u{2014} do not edit\n");
    out.push_str("// Regenerate: flux generate\n");
    out.push_str("package flux.generated;\n\n");
    out.push_str("import java.util.UUID;\n");
    out.push_str("import java.time.Instant;\n\n");
    out.push_str("// \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");

    for (table, schema) in &manifest.database {
        let fields: Vec<String> = schema.columns.iter().map(|col| {
            let java_type = pg_to_java(&col.col_type);
            // Box primitives for nullable columns
            let actual_type = if col.nullable {
                match java_type {
                    "int"     => "Integer",
                    "long"    => "Long",
                    "boolean" => "Boolean",
                    "double"  => "Double",
                    other     => other,
                }
            } else {
                java_type
            };
            format!("{} {}", actual_type, col.name)
        }).collect();
        out.push_str(&format!("public record {}({}) {{}}\n\n", to_pascal_case(table), fields.join(", ")));
    }

    if !manifest.secrets.is_empty() {
        out.push_str("// \u{2500}\u{2500} Secret key constants \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
        out.push_str("public interface SecretKeys {\n");
        for key in &manifest.secrets {
            let const_name = key.to_uppercase().replace('-', "_");
            out.push_str(&format!("    String {} = \"{}\";\n", const_name, key));
        }
        out.push_str("}\n\n");
    }

    out.push_str("// \u{2500}\u{2500} Context interface \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    out.push_str("public interface FluxDb {}\n");
    out.push_str("public interface FluxFunctions {}\n");
    out.push_str("public interface FluxSecrets {}\n");
    out.push_str("public interface FluxAgents {}\n\n");
    out.push_str("public interface FluxContext {\n");
    out.push_str("    FluxDb db();\n");
    out.push_str("    FluxFunctions functions();\n");
    out.push_str("    FluxSecrets secrets();\n");
    out.push_str("    FluxAgents agents();\n");
    out.push_str("}\n");

    let path = out_dir.join("Ctx.java");
    std::fs::write(&path, out)?;
    Ok(path)
}

// ── Generator 14: Ruby ────────────────────────────────────────────────────────

fn generate_ruby(manifest: &Manifest, out_dir: &Path) -> anyhow::Result<PathBuf> {
    let mut out = String::new();
    out.push_str("# Generated by flux generate \u{2014} do not edit\n");
    out.push_str("# Regenerate: flux generate\n");
    out.push_str("# frozen_string_literal: true\n\n");
    out.push_str("module Flux\n");
    out.push_str("  module Generated\n");
    out.push_str("    # \u{2500}\u{2500} Database row types \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    for (table, schema) in &manifest.database {
        let fields: Vec<String> = schema.columns.iter().map(|c| format!(":{}", c.name)).collect();
        out.push_str(&format!(
            "    {} = Struct.new({}, keyword_init: true)\n\n",
            to_pascal_case(table),
            fields.join(", ")
        ));
    }

    if !manifest.secrets.is_empty() {
        out.push_str("    # \u{2500}\u{2500} Secret key constants \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
        out.push_str("    module SecretKeys\n");
        for key in &manifest.secrets {
            let const_name = key.to_uppercase().replace('-', "_");
            out.push_str(&format!("      {} = \"{}\"\n", const_name, key));
        }
        out.push_str("    end\n\n");
    }

    out.push_str("    # \u{2500}\u{2500} Context interface \u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\n\n");
    out.push_str("    module FluxContext\n");
    out.push_str("      # @return [Object]\n");
    out.push_str("      def db = raise NotImplementedError\n");
    out.push_str("      # @return [Object]\n");
    out.push_str("      def functions = raise NotImplementedError\n");
    out.push_str("      # @return [Object]\n");
    out.push_str("      def secrets = raise NotImplementedError\n");
    out.push_str("      # @return [Object]\n");
    out.push_str("      def agents = raise NotImplementedError\n");
    out.push_str("    end\n");
    out.push_str("  end\n");
    out.push_str("end\n");

    let path = out_dir.join("ctx.rb");
    std::fs::write(&path, out)?;
    Ok(path)
}
