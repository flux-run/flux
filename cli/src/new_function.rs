//! `flux new function <name> [--language <lang>]`
//!
//! Scaffolds a new function inside the `functions/` directory.
//!
//! ```text
//! $ flux new function create_user --language typescript
//!
//! ◆ Scaffolding function create_user (typescript)
//!
//!   ✔ functions/create_user/index.ts
//!   ✔ functions/create_user/flux.json
//!
//!   Next steps:
//!     1. Edit functions/create_user/index.ts
//!     2. flux deploy
//! ```
//!
//! Supported languages match the 14 runtimes in `flux generate`:
//! TypeScript, JavaScript, Rust, Go, Python, C, C++, Zig,
//! AssemblyScript, C#, Swift, Kotlin, Java, Ruby
//!
//! ## SOLID
//!
//! - SRP: each language has its own `scaffold_<lang>()` fn returning (filename, content).
//! - OCP: add a new language by adding one match arm + one fn — nothing else changes.
//! - DIP: `execute_new_function` calls `scaffold_for_language()` which is the single
//!   dispatch point.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context as _};
use colored::Colorize;

// ── Supported languages ───────────────────────────────────────────────────────

/// Languages available via `--language`.
/// The first name in each list is canonical; extras are accepted aliases.
const LANGUAGES: &[(&str, &[&str])] = &[
    ("typescript",    &["ts"]),
    ("javascript",    &["js", "deno"]),
    ("rust",          &["rs"]),
    ("go",            &["golang"]),
    ("python",        &["py"]),
    ("c",             &[]),
    ("cpp",           &["c++", "cxx"]),
    ("zig",           &[]),
    ("assemblyscript",&["as"]),
    ("csharp",        &["cs", "c#", "dotnet"]),
    ("swift",         &[]),
    ("kotlin",        &["kt"]),
    ("java",          &[]),
    ("ruby",          &["rb"]),
];

/// Resolve an alias (e.g. "ts" → "typescript").  Returns an error listing
/// valid choices if the language is not recognised.
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

// ── Entry point ───────────────────────────────────────────────────────────────

/// `flux new function <name> [--language <lang>]`
pub fn execute_new_function(name: String, language: Option<String>) -> anyhow::Result<()> {
    // Validate name — must be a valid identifier (snake_case recommended).
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

    // Generate the two files.
    let (code_file, code_content) = scaffold_for_language(lang, &name);
    let flux_json = scaffold_flux_json(&name, lang);

    write_file(&fn_dir, code_file, &code_content)?;
    write_file(&fn_dir, "flux.json", &flux_json)?;

    // Write any language-specific extra files (package.json, tsconfig, Cargo.toml…)
    let extras = scaffold_extra_files(lang, &name);
    for (extra_file, extra_content) in &extras {
        write_file(&fn_dir, extra_file, extra_content)?;
    }

    // Print file list.
    println!(
        "  {} functions/{}/{}",
        "✔".green().bold(),
        name,
        code_file
    );
    println!(
        "  {} functions/{}/flux.json",
        "✔".green().bold(),
        name
    );
    for (extra_file, _) in &extras {
        println!(
            "  {} functions/{}/{}",
            "✔".green().bold(),
            name,
            extra_file
        );
    }
    println!();
    println!("  {}", "Next steps:".bold());
    println!(
        "    1.  Edit {}",
        format!("functions/{}/{}", name, code_file).cyan()
    );
    println!("    2.  {}", "flux deploy".cyan());
    println!();

    Ok(())
}

fn write_file(dir: &Path, filename: &str, content: &str) -> anyhow::Result<()> {
    let path = dir.join(filename);
    // Ensure any intermediate subdirectories exist (e.g. "src/" for Rust).
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory {}", parent.display()))?;
    }
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write {}", path.display()))
}

// ── flux.json ─────────────────────────────────────────────────────────────────

fn scaffold_flux_json(name: &str, lang: &str) -> String {
    let (runtime, entry) = lang_runtime_entry(lang, name);
    format!(
        r#"{{
  "name": "{name}",
  "runtime": "{runtime}",
  "entry": "{entry}",
  "description": "TODO: describe what {name} does",
  "schema": {{
    "input": {{
      "type": "object",
      "required": [],
      "properties": {{
      }}
    }},
    "output": {{
      "type": "object",
      "properties": {{
      }}
    }}
  }}
}}
"#,
        name = name,
        runtime = runtime,
        entry = entry
    )
}

fn lang_runtime_entry(lang: &str, name: &str) -> (&'static str, String) {
    match lang {
        "typescript"     => ("deno",     "index.ts".into()),
        "javascript"     => ("deno",     "index.js".into()),
        "rust"           => ("wasm",     format!("{}.wasm", name)),
        "go"             => ("wasm",     format!("{}.wasm", name)),
        "python"         => ("wasm",     format!("{}.wasm", name)),
        "c"              => ("wasm",     format!("{}.wasm", name)),
        "cpp"            => ("wasm",     format!("{}.wasm", name)),
        "zig"            => ("wasm",     format!("{}.wasm", name)),
        "assemblyscript" => ("wasm",     "index.wasm".into()),
        "csharp"         => ("wasm",     format!("{}.wasm", name)),
        "swift"          => ("wasm",     format!("{}.wasm", name)),
        "kotlin"         => ("wasm",     format!("{}.wasm", name)),
        "java"           => ("wasm",     format!("{}.wasm", name)),
        "ruby"           => ("wasm",     format!("{}.wasm", name)),
        _                => ("deno",     "index.ts".into()),
    }
}

// ── Per-language scaffolds ────────────────────────────────────────────────────

/// Returns `(filename, file_content)` for the function code file.
fn scaffold_for_language(lang: &str, name: &str) -> (&'static str, String) {
    match lang {
        "typescript"     => ("index.ts",       scaffold_typescript(name)),
        "javascript"     => ("index.js",       scaffold_javascript(name)),
        "rust"           => ("src/lib.rs",     scaffold_rust(name)),
        "go"             => ("main.go",        scaffold_go(name)),
        "python"         => ("handler.py",     scaffold_python(name)),
        "c"              => ("handler.c",      scaffold_c(name)),
        "cpp"            => ("handler.cpp",    scaffold_cpp(name)),
        "zig"            => ("handler.zig",    scaffold_zig(name)),
        "assemblyscript" => ("index.ts",       scaffold_assemblyscript(name)),
        "csharp"         => ("Handler.cs",     scaffold_csharp(name)),
        "swift"          => ("Handler.swift",  scaffold_swift(name)),
        "kotlin"         => ("Handler.kt",     scaffold_kotlin(name)),
        "java"           => ("Handler.java",   scaffold_java(name)),
        "ruby"           => ("handler.rb",     scaffold_ruby(name)),
        _                => ("index.ts",       scaffold_typescript(name)),
    }
}

/// Returns extra files to write alongside the main code file.
/// Every language gets its build configuration — no manual setup required.
fn scaffold_extra_files(lang: &str, name: &str) -> Vec<(&'static str, String)> {
    match lang {
        "typescript" => vec![
            ("@fluxbase-functions.ts", FLUX_SDK_SHIM.to_string()),
            ("tsconfig.json",          scaffold_tsconfig()),
        ],
        "javascript" => vec![
            ("@fluxbase-functions.js",     flux_sdk_shim_js()),
            ("@fluxbase-functions.d.ts",   FLUX_SDK_SHIM.to_string()),
            ("tsconfig.json",              scaffold_tsconfig_js()),
        ],
        "assemblyscript" => vec![
            ("@fluxbase-functions.ts", FLUX_SDK_SHIM.to_string()),
            ("tsconfig.json",          scaffold_tsconfig_assemblyscript()),
            ("asconfig.json",          scaffold_asconfig(name)),
            ("assembly.d.ts",          scaffold_assembly_dts()),
        ],
        "rust" => vec![
            ("Cargo.toml", scaffold_rust_cargo(name)),
        ],
        "go" => vec![
            ("go.mod", scaffold_go_mod(name)),
        ],
        "python" => vec![
            ("requirements.txt", scaffold_python_requirements()),
        ],
        "c" => vec![
            ("Makefile", scaffold_c_makefile(name)),
        ],
        "cpp" => vec![
            ("Makefile", scaffold_cpp_makefile(name)),
        ],
        "zig" => vec![
            ("build.zig", scaffold_zig_build(name)),
        ],
        "csharp" => vec![
            ("Handler.csproj", scaffold_csharp_csproj(name)),
        ],
        "swift" => vec![
            ("Package.swift", scaffold_swift_package(name)),
        ],
        "kotlin" => vec![
            ("build.gradle.kts",    scaffold_kotlin_gradle(name)),
            ("settings.gradle.kts", scaffold_kotlin_settings(name)),
        ],
        "java" => vec![
            ("build.gradle",    scaffold_java_gradle(name)),
            ("settings.gradle", scaffold_java_settings(name)),
        ],
        "ruby" => vec![
            ("Gemfile", scaffold_ruby_gemfile()),
        ],
        _ => vec![],
    }
}

fn pascal(name: &str) -> String {
    name.split(['_', '-'])
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None    => String::new(),
                Some(f) => f.to_uppercase().to_string() + c.as_str(),
            }
        })
        .collect()
}

// ─── TypeScript ───────────────────────────────────────────────────────────────

fn scaffold_typescript(name: &str) -> String {
    format!(
        r#"import {{ defineFunction }} from "@fluxbase/functions";

export default defineFunction({{
  name: "{name}",
  handler: async ({{ input, ctx }}) => {{
    ctx.log("Running {name}");

    // ctx.db.<table>.find({{ where: ... }})   — query your database
    // ctx.secrets.get("MY_SECRET")            — read a secret
    // ctx.functions.<other>(input)            — call another function

    return {{
      ok: true,
    }};
  }},
}});
"#,
        name = name
    )
}

// ─── JavaScript ───────────────────────────────────────────────────────────────

fn scaffold_javascript(name: &str) -> String {
    format!(
        r#"import {{ defineFunction }} from "@fluxbase/functions";

export default defineFunction({{
  name: "{name}",
  /** @param {{ input: any, ctx: import("@fluxbase/functions").FluxContext }} args */
  handler: async ({{ input, ctx }}) => {{
    ctx.log("Running {name}");

    return {{ ok: true }};
  }},
}});
"#,
        name = name
    )
}

// ─── @fluxbase/functions SDK shim (TypeScript) ────────────────────────────────
//
// Embedded at compile-time so no `npm install` is ever needed.
// `tsconfig.json` maps `@fluxbase/functions` → `./@fluxbase-functions`
// giving editors full IntelliSense and the bundler a local resolution target.

const FLUX_SDK_SHIM: &str = r#"// @fluxbase/functions — embedded by the Flux CLI (no npm install needed)
// Do not edit this file; it is regenerated on `flux function create`.

export interface Schema<T = unknown> {
  parse(data: unknown): T;
  safeParse(data: unknown): { success: true; data: T } | { success: false; error: unknown };
}

export interface FluxSecrets {
  get(key: string): string | null;
}

export interface FluxTools {
  run(toolName: string, input: Record<string, unknown>): Promise<Record<string, unknown>>;
}

export interface FluxWorkflow {
  run(
    steps: Array<{ name: string; fn: (ctx: FluxContext, previous: Record<string, unknown>) => Promise<unknown> }>,
    options?: { continueOnError?: boolean },
  ): Promise<Record<string, unknown>>;
  parallel(
    steps: Array<{ name: string; fn: (ctx: FluxContext) => Promise<unknown> }>,
  ): Promise<Record<string, unknown>>;
}

export interface FluxAgentResult {
  answer: string;
  steps: number;
  output: Record<string, unknown> | null;
}

export interface FluxAgent {
  run(options: { goal: string; tools?: string[]; maxSteps?: number }): Promise<FluxAgentResult>;
}

export interface FluxContext {
  payload: unknown;
  secrets: FluxSecrets;
  env: Record<string, string>;
  log(message: string, level?: "info" | "warn" | "error"): void;
  tools: FluxTools;
  workflow: FluxWorkflow;
  agent: FluxAgent;
  db: Record<string, {
    find(where?: Record<string, unknown>): Promise<unknown[]>;
    findOne(where?: Record<string, unknown>): Promise<unknown | null>;
    insert(data: Record<string, unknown>): Promise<unknown>;
    update(where: Record<string, unknown>, data: Record<string, unknown>): Promise<unknown[]>;
    delete(where: Record<string, unknown>): Promise<void>;
  }>;
  functions: Record<string, (input: unknown) => Promise<unknown>>;
}

export interface HandlerArgs<TInput = unknown> {
  input: TInput;
  ctx: FluxContext;
}

export interface DefineFunctionOptions<TInput = unknown, TOutput = unknown> {
  name: string;
  description?: string;
  input?: Schema<TInput>;
  output?: Schema<TOutput>;
  handler: (args: HandlerArgs<TInput>) => Promise<TOutput>;
}

export interface FunctionDefinition<TInput = unknown, TOutput = unknown> {
  readonly __fluxbase: true;
  readonly metadata: {
    name: string;
    description?: string;
    input_schema: Record<string, unknown> | null;
    output_schema: Record<string, unknown> | null;
  };
  execute(payload: unknown, context: FluxContext): Promise<TOutput>;
}

export function defineFunction<TInput = unknown, TOutput = unknown>(
  options: DefineFunctionOptions<TInput, TOutput>,
): FunctionDefinition<TInput, TOutput> {
  const { name, description, input: inputSchema, output: outputSchema, handler } = options;
  return {
    __fluxbase: true,
    metadata: { name, description: description, input_schema: null, output_schema: null },
    async execute(payload: unknown, context: FluxContext): Promise<TOutput> {
      const input = inputSchema ? inputSchema.parse(payload) : (payload as TInput);
      const output = await handler({ input, ctx: context });
      if (outputSchema) outputSchema.parse(output);
      return output;
    },
  };
}
"#;

fn flux_sdk_shim_js() -> String {
    // Runtime implementation for JavaScript — types come from @fluxbase-functions.d.ts
    r#"// @fluxbase/functions — embedded by the Flux CLI (no npm install needed)
// Do not edit this file; it is regenerated on `flux function create`.
// Types are in @fluxbase-functions.d.ts (auto-loaded by tsconfig.json).

/**
 * @template TInput, TOutput
 * @param {import("./@fluxbase-functions").DefineFunctionOptions<TInput, TOutput>} options
 * @returns {import("./@fluxbase-functions").FunctionDefinition<TInput, TOutput>}
 */
export function defineFunction(options) {
  const { name, description, input: inputSchema, output: outputSchema, handler } = options;
  return {
    __fluxbase: true,
    metadata: { name, description, input_schema: null, output_schema: null },
    async execute(payload, context) {
      const input = inputSchema ? inputSchema.parse(payload) : payload;
      const output = await handler({ input, ctx: context });
      if (outputSchema) outputSchema.parse(output);
      return output;
    },
  };
}
"#
    .to_string()
}

fn scaffold_tsconfig() -> String {
    r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "skipLibCheck": true,
    "paths": {
      "@fluxbase/functions": ["./@fluxbase-functions"]
    }
  }
}
"#
    .to_string()
}

fn scaffold_tsconfig_js() -> String {
    r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "allowJs": true,
    "checkJs": true,
    "strict": true,
    "skipLibCheck": true,
    "noEmit": true,
    "paths": {
      "@fluxbase/functions": ["./@fluxbase-functions"]
    }
  },
  "include": ["*.js"]
}
"#
    .to_string()
}

fn scaffold_tsconfig_assemblyscript() -> String {
    // Use ESNext lib (not empty) — AS types are additive via assembly.d.ts.
    // "files" ensures assembly.d.ts is always loaded before the source.
    r#"{
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "strict": true,
    "skipLibCheck": true,
    "lib": ["ESNext"],
    "paths": {
      "@fluxbase/functions": ["./@fluxbase-functions"]
    }
  },
  "files": ["assembly.d.ts", "index.ts"]
}
"#
    .to_string()
}

fn scaffold_assembly_dts() -> String {
    r#"// AssemblyScript built-in declarations for editor/tsc support.
// The real AS compiler (asc) has these built-in; this file enables IntelliSense.
// Do not edit — regenerated on `flux function create`.

declare type i8   = number;
declare type i16  = number;
declare type i32  = number;
declare type i64  = number;
declare type isize = number;
declare type u8   = number;
declare type u16  = number;
declare type u32  = number;
declare type u64  = number;
declare type usize = number;
declare type f32  = number;
declare type f64  = number;
declare type bool = boolean;
declare type v128 = never;

/** Reinterpret the bits of a value as a different type (AssemblyScript built-in). */
declare function changetype<T>(value: unknown): T;
declare function unreachable(): never;

// Augment the global String constructor to add AssemblyScript's UTF8 helpers.
interface StringConstructor {
  UTF8: {
    encode(str: string, nullTerminated?: bool): ArrayBuffer;
    decode(buf: ArrayBuffer | Uint8Array, nullTerminated?: bool): string;
    byteLength(str: string, nullTerminated?: bool): i32;
  };
}
"#
    .to_string()
}

fn scaffold_rust_cargo(name: &str) -> String {
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
serde       = {{ version = "1", features = ["derive"] }}
serde_json  = "1"

[profile.release]
opt-level = "z"
lto       = true

# Opt out of the Flux monorepo workspace — this is a standalone project.
[workspace]
"#,
        name = name
    )
}

fn scaffold_go_mod(name: &str) -> String {
    format!(
        r#"module {name}

go 1.21

// Build: GOOS=wasip1 GOARCH=wasm go build -o {name}.wasm .
// Requires: Go 1.21+  https://go.dev/doc/install
"#,
        name = name
    )
}

fn scaffold_python_requirements() -> String {
    r#"# Python WASM functions have no external dependencies by default.
# Add pure-Python packages here — they will be bundled automatically.
# Note: C-extension packages are not supported in WASM.
#
# Build: flux deploy  (the CLI compiles handler.py → WASM via py2wasm)
# Install py2wasm: pip install py2wasm
"#
    .to_string()
}

fn scaffold_c_makefile(name: &str) -> String {
    format!(
        r#"# Build {name} → WASM using wasi-sdk
# Install: https://github.com/WebAssembly/wasi-sdk/releases
WASI_SDK ?= /opt/wasi-sdk
CC        = $(WASI_SDK)/bin/clang

CFLAGS = --target=wasm32-wasi          \
         -nostdlib                      \
         -Wl,--no-entry                 \
         -Wl,--export={name}_handler   \
         -Wl,--allow-undefined

.PHONY: build clean

build: {name}.wasm

{name}.wasm: handler.c
	$(CC) $(CFLAGS) -O2 -o $@ $<

clean:
	rm -f {name}.wasm
"#,
        name = name
    )
}

fn scaffold_cpp_makefile(name: &str) -> String {
    format!(
        r#"# Build {name} → WASM using wasi-sdk
# Install: https://github.com/WebAssembly/wasi-sdk/releases
WASI_SDK ?= /opt/wasi-sdk
CXX       = $(WASI_SDK)/bin/clang++

CXXFLAGS = --target=wasm32-wasi        \
           -nostdlib                    \
           -fno-exceptions             \
           -Wl,--no-entry              \
           -Wl,--export={name}_handler \
           -Wl,--allow-undefined

.PHONY: build clean

build: {name}.wasm

{name}.wasm: handler.cpp
	$(CXX) $(CXXFLAGS) -O2 -o $@ $<

clean:
	rm -f {name}.wasm
"#,
        name = name
    )
}

fn scaffold_zig_build(name: &str) -> String {
    format!(
        r#"// Build {name} → WASM
// Install Zig >= 0.12: https://ziglang.org/download/
const std = @import("std");

pub fn build(b: *std.Build) void {{
    const lib = b.addSharedLibrary(.{{
        .name = "{name}",
        .root_source_file = b.path("handler.zig"),
        .target = b.resolveTargetQuery(.{{
            .cpu_arch = .wasm32,
            .os_tag   = .wasi,
        }}),
        .optimize = .ReleaseSmall,
    }});
    lib.rdynamic = true;
    b.installArtifact(lib);
}}
"#,
        name = name
    )
}

fn scaffold_asconfig(name: &str) -> String {
    format!(
        r#"{{
  "targets": {{
    "release": {{
      "outFile": "{name}.wasm",
      "sourceMap": false,
      "optimize": true,
      "runtime": "stub"
    }}
  }},
  "options": {{
    "exportRuntime": false
  }}
}}
"#,
        name = name
    )
}

fn scaffold_csharp_csproj(name: &str) -> String {
    format!(
        r#"<!-- Build: dotnet build -c Release
     Install WASI workload: dotnet workload install wasi-experimental -->
<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <AssemblyName>{name}</AssemblyName>
    <OutputType>Exe</OutputType>
    <TargetFramework>net9.0</TargetFramework>
    <RuntimeIdentifier>wasi-wasm</RuntimeIdentifier>
    <UseAppHost>false</UseAppHost>
    <AllowUnsafeBlocks>true</AllowUnsafeBlocks>
    <Optimize>true</Optimize>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Newtonsoft.Json" Version="13.*" />
  </ItemGroup>
</Project>
"#,
        name = name
    )
}

fn scaffold_swift_package(name: &str) -> String {
    format!(
        r#"// swift-tools-version: 5.9
// Build: swift build -c release --triple wasm32-unknown-wasi
// Install SwiftWasm toolchain: https://swiftwasm.org
import PackageDescription

let package = Package(
    name: "{name}",
    targets: [
        .executableTarget(
            name: "{name}",
            path: ".",
            swiftSettings: [
                .unsafeFlags(["-target", "wasm32-unknown-wasi"]),
            ]
        ),
    ]
)
"#,
        name = name
    )
}

fn scaffold_kotlin_gradle(name: &str) -> String {
    let _ = name; // name used only in settings.gradle.kts
    r#"// Build: ./gradlew wasmWasiBinaries
// Install: Kotlin >= 1.9 — https://kotlinlang.org/docs/wasm-get-started.html
plugins {
    kotlin("multiplatform") version "2.0.0"
}

kotlin {
    wasmWasi {
        binaries.executable()
    }

    sourceSets {
        val wasmWasiMain by getting
    }
}
"#
    .to_string()
}

fn scaffold_kotlin_settings(name: &str) -> String {
    format!(
        r#"rootProject.name = "{name}"
"#,
        name = name
    )
}

fn scaffold_java_gradle(name: &str) -> String {
    format!(
        r#"// Build: gradle nativeCompile   (requires GraalVM JDK)
// Install GraalVM: https://www.graalvm.org/downloads/
plugins {{
    id 'java'
    id 'org.graalvm.buildtools.native' version '0.10.1'
}}

group   = 'dev.fluxbase'
version = '0.1.0'

java {{
    sourceCompatibility = JavaVersion.VERSION_21
    targetCompatibility = JavaVersion.VERSION_21
}}

repositories {{
    mavenCentral()
}}

dependencies {{
    // GraalVM Native Image API — needed for @CEntryPoint annotation
    compileOnly 'org.graalvm.sdk:graal-sdk:23.1.4'
}}

graalvmNative {{
    binaries {{
        main {{
            imageName = '{name}'
            buildArgs.add('--no-fallback')
            buildArgs.add('-H:Kind=SHARED_LIBRARY')
        }}
    }}
}}
"#,
        name = name
    )
}

fn scaffold_java_settings(name: &str) -> String {
    format!(
        r#"rootProject.name = '{name}'
"#,
        name = name
    )
}

fn scaffold_ruby_gemfile() -> String {
    r#"# frozen_string_literal: true
source "https://rubygems.org"

# Ruby WASM runtime — bundles Ruby + your handler into a .wasm file.
# Build: flux deploy  (the CLI runs: ruby.wasm build handler.rb -o <name>.wasm)
# Install: gem install ruby_wasm
gem "ruby_wasm", "~> 2.0"
"#
    .to_string()
}

// ─── Rust ─────────────────────────────────────────────────────────────────────

fn scaffold_rust(name: &str) -> String {
    format!(
        r#"use serde::{{Deserialize, Serialize}};

// Input and output types are validated against flux.json "schema" at the gateway.
#[derive(Deserialize)]
pub struct Input {{
    // TODO: add your input fields
}}

#[derive(Serialize)]
pub struct Output {{
    ok: bool,
}}

#[no_mangle]
pub extern "C" fn {name}_handler(ptr: i32, len: i32) -> i64 {{
    // Flux calls this function with JSON-encoded input.
    let input_bytes = unsafe {{ std::slice::from_raw_parts(ptr as *const u8, len as usize) }};
    let _input: Input = serde_json::from_slice(input_bytes).unwrap();

    let output = Output {{ ok: true }};
    let out_bytes = serde_json::to_vec(&output).unwrap();

    // Return pointer+length packed into an i64.
    let out_ptr = out_bytes.as_ptr() as i64;
    let out_len = out_bytes.len() as i64;
    std::mem::forget(out_bytes);
    (out_ptr << 32) | out_len
}}
"#,
        name = name,
    )
}

// ─── Go ───────────────────────────────────────────────────────────────────────

fn scaffold_go(name: &str) -> String {
    let _ = name;
    r#"//go:build wasip1

package main

import (
	"encoding/json"
	"os"
)

type Input struct {
	// TODO: add your input fields
}

type Output struct {
	OK bool `json:"ok"`
}

func main() {
	var input Input
	if err := json.NewDecoder(os.Stdin).Decode(&input); err != nil {
		os.Exit(1)
	}
	out := Output{OK: true}
	if err := json.NewEncoder(os.Stdout).Encode(out); err != nil {
		os.Exit(1)
	}
}
"#
    .to_string()
}

// ─── Python ───────────────────────────────────────────────────────────────────

fn scaffold_python(name: &str) -> String {
    format!(
        r#"# {name} — Flux function (compiled to WASM via py2wasm)
# Build: py2wasm -i handler.py -o {name}.wasm

import json

def handler(input_json: str) -> str:
    '''Entry point called by the Flux runtime.'''
    payload = json.loads(input_json)

    # TODO: implement {name}
    result = {{"ok": True}}

    return json.dumps(result)
"#,
        name = name
    )
}

// ─── C ────────────────────────────────────────────────────────────────────────

fn scaffold_c(name: &str) -> String {
    format!(
        r#"// {name} — Flux function (compiled to WASM via wasi-sdk)
// Build: see Makefile  (requires wasi-sdk — https://github.com/WebAssembly/wasi-sdk)
#include <stdint.h>

// Static response in WASM linear memory.
static const char RESP[] = "{{\"ok\":true}}";
#define RESP_LEN ((uint32_t)(sizeof(RESP) - 1))

__attribute__((export_name("{name}_handler")))
uint64_t {name}_handler(uint32_t input_ptr, uint32_t input_len) {{
    (void)input_ptr;
    (void)input_len;
    // Return (pointer << 32) | length packed into uint64.
    return ((uint64_t)(uintptr_t)RESP << 32) | RESP_LEN;
}}
"#,
        name = name
    )
}

// ─── C++ ──────────────────────────────────────────────────────────────────────

fn scaffold_cpp(name: &str) -> String {
    format!(
        r#"// {name} — Flux function (compiled to WASM via wasi-sdk)
// Build: see Makefile  (requires wasi-sdk — https://github.com/WebAssembly/wasi-sdk)
#include <cstdint>

static const char RESP[] = "{{\"ok\":true}}";
constexpr uint32_t RESP_LEN = sizeof(RESP) - 1;

extern "C" {{
    __attribute__((export_name("{name}_handler")))
    uint64_t {name}_handler(uint32_t input_ptr, uint32_t input_len) {{
        (void)input_ptr;
        (void)input_len;
        return (static_cast<uint64_t>(reinterpret_cast<uintptr_t>(RESP)) << 32) | RESP_LEN;
    }}
}}
"#,
        name = name
    )
}

// ─── Zig ──────────────────────────────────────────────────────────────────────

fn scaffold_zig(name: &str) -> String {
    format!(
        r#"// {name} — Flux function (compiled to WASM via zig build-lib)
// Build: zig build-lib handler.zig -target wasm32-freestanding -dynamic -o {name}.wasm
const std = @import("std");

var output_buf: [1024]u8 = undefined;

export fn {name}_handler(input_ptr: [*]const u8, input_len: u32) u64 {{
    _ = input_ptr;
    _ = input_len;

    const resp = "{{\"ok\":true}}";
    @memcpy(output_buf[0..resp.len], resp);

    const ptr: u64 = @intFromPtr(&output_buf);
    return (ptr << 32) | resp.len;
}}
"#,
        name = name
    )
}

// ─── AssemblyScript ───────────────────────────────────────────────────────────

fn scaffold_assemblyscript(name: &str) -> String {
    format!(
        r#"// {name} — Flux function (AssemblyScript → WASM)
// Build: asc index.ts --target release -o index.wasm

export function {name}_handler(input_ptr: i32, input_len: i32): i64 {{
  // TODO: decode input JSON at input_ptr with length input_len
  const resp = `{{"ok":true}}`;
  const buf  = String.UTF8.encode(resp);
  const ptr  = changetype<i32>(buf);
  return (<i64>ptr << 32) | buf.byteLength;
}}
"#,
        name = name
    )
}

// ─── C# ───────────────────────────────────────────────────────────────────────

fn scaffold_csharp(name: &str) -> String {
    let pascal = pascal(name);
    format!(
        r#"// {name} — Flux function (compiled to WASM via dotnet wasi)
// Requires: dotnet add package Wasi.Sdk
using System.Text.Json;
using System.Runtime.InteropServices;

public static class {pascal}Handler
{{
    [UnmanagedCallersOnly(EntryPoint = "{name}_handler")]
    public static long Handle(IntPtr inputPtr, int inputLen)
    {{
        // TODO: decode input, run logic
        var output = JsonSerializer.SerializeToUtf8Bytes(new {{ ok = true }});
        var outPtr = Marshal.AllocHGlobal(output.Length);
        Marshal.Copy(output, 0, outPtr, output.Length);
        return ((long)outPtr << 32) | output.Length;
    }}
}}
"#,
        name = name,
        pascal = pascal
    )
}

// ─── Swift ────────────────────────────────────────────────────────────────────

fn scaffold_swift(name: &str) -> String {
    let pascal = pascal(name);
    format!(
        "// {name} — Flux function (compiled to WASM via swiftwasm)\n\
// Build: swiftc -target wasm32-unknown-wasi Handler.swift -o {name}.wasm\n\
\n\
@_cdecl(\"{name}_handler\")\n\
func {pascal}Handler(inputPtr: UnsafePointer<UInt8>, inputLen: UInt32) -> UInt64 {{\n\
    // TODO: decode JSON at inputPtr\n\
    let response = #\"{{\\\"ok\\\":true}}\"#\n\
    let bytes    = Array(response.utf8)\n\
    let outPtr   = UnsafeMutablePointer<UInt8>.allocate(capacity: bytes.count)\n\
    outPtr.initialize(from: bytes, count: bytes.count)\n\
    return (UInt64(UInt(bitPattern: outPtr)) << 32) | UInt64(bytes.count)\n\
}}\n",
        name = name,
        pascal = pascal
    )
}

// ─── Kotlin ───────────────────────────────────────────────────────────────────

fn scaffold_kotlin(name: &str) -> String {
    let pascal = pascal(name);
    format!(
        r#"// {name} — Flux function (compiled to WASM via Kotlin/Wasm)
// Build: ./gradlew wasmWasiJar
import kotlinx.serialization.json.*

@Suppress("unused")
@WasmExport("{name}_handler")
fun {pascal}Handler(inputPtr: Int, inputLen: Int): Long {{
    // TODO: decode input JSON at inputPtr
    val response = """{{ "ok": true }}"""
    val bytes    = response.encodeToByteArray()
    // Allocation: host is responsible for freeing.
    return (inputPtr.toLong() shl 32) or bytes.size.toLong()
}}
"#,
        name = name,
        pascal = pascal
    )
}

// ─── Java ─────────────────────────────────────────────────────────────────────

fn scaffold_java(name: &str) -> String {
    format!(
        r#"// {name} — Flux function
// Build to WASM: ./gradlew nativeCompile  (see build.gradle — requires GraalVM JDK)
// Type-check:    javac Handler.java        (works with standard JDK 21+)
import java.nio.charset.StandardCharsets;

public class Handler {{

    /**
     * Flux runtime entry point — exported as "{name}_handler" in the WASM binary.
     *
     * When using GraalVM Native Image, annotate with @CEntryPoint and accept an
     * IsolateThread as the first argument. For development and type-checking, this
     * plain static method compiles with any standard JDK.
     *
     * @param inputPtr  pointer to JSON-encoded input in WASM linear memory
     * @param inputLen  byte length of the input
     * @return          (outputPtr &lt;&lt; 32) | outputLen
     */
    public static long handle(long inputPtr, int inputLen) {{
        // TODO: decode JSON input from linear memory at inputPtr
        byte[] resp = "{{\"ok\":true}}".getBytes(StandardCharsets.UTF_8);
        // Return (outputPtr << 32) | outputLen packed into a long.
        return ((long) inputPtr << 32) | resp.length;
    }}

    public static void main(String[] args) {{
        // Local smoke-test
        System.out.println(handle(0, 0));
    }}
}}
"#,
        name = name,
    )
}

// ─── Ruby ─────────────────────────────────────────────────────────────────────

fn scaffold_ruby(name: &str) -> String {
    format!(
        r#"# {name} — Flux function (compiled to WASM via ruby.wasm)
# Build: ruby.wasm build handler.rb -o {name}.wasm
require 'json'

# @param input_json [String]  JSON-encoded input payload
# @return [String]            JSON-encoded output
def {name}_handler(input_json)
  _input = JSON.parse(input_json)

  # TODO: implement {name}

  JSON.generate(ok: true)
end
"#,
        name = name
    )
}
