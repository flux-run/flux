use clap::{Subcommand, ValueEnum};
use crate::client::ApiClient;
use serde_json::Value;
use std::fs;
use std::path::Path;

// ── Language enum ─────────────────────────────────────────────────────────────

#[derive(Clone, Debug, ValueEnum)]
pub enum Language {
    /// TypeScript / JavaScript — runs on Deno (no WASM compile step)
    Typescript,
    /// Rust — compiled to WASM via `cargo build --target wasm32-wasip1`
    Rust,
    /// Go — compiled to WASM via TinyGo
    Go,
    /// AssemblyScript — compiled via `npx asc`
    Assemblyscript,
    /// C / C++ — compiled via wasi-sdk `clang`
    C,
    /// Zig — compiled with `zig build`
    Zig,
    /// Python — compiled via py2wasm
    Python,
}

impl Language {
    fn as_str(&self) -> &'static str {
        match self {
            Language::Typescript    => "typescript",
            Language::Rust          => "rust",
            Language::Go            => "go",
            Language::Assemblyscript => "assemblyscript",
            Language::C             => "c",
            Language::Zig           => "zig",
            Language::Python        => "python",
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
            scaffold(&name, &language)?;
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

// ── Scaffold dispatch ─────────────────────────────────────────────────────────

fn scaffold(name: &str, language: &Language) -> anyhow::Result<()> {
    let dir = Path::new(name);
    if dir.exists() {
        anyhow::bail!("Directory '{}' already exists.", name);
    }
    fs::create_dir_all(dir)?;

    match language {
        Language::Typescript    => scaffold_typescript(dir, name)?,
        Language::Rust          => scaffold_rust(dir, name)?,
        Language::Go            => scaffold_go(dir, name)?,
        Language::Assemblyscript => scaffold_assemblyscript(dir, name)?,
        Language::C             => scaffold_c(dir, name)?,
        Language::Zig           => scaffold_zig(dir, name)?,
        Language::Python        => scaffold_python(dir, name)?,
    }

    println!("\n✅  Created '{}' ({})\n", name, language.as_str());
    print_next_steps(name, language);
    Ok(())
}

// ── TypeScript ────────────────────────────────────────────────────────────────

fn scaffold_typescript(dir: &Path, name: &str) -> anyhow::Result<()> {
    let flux_json = serde_json::json!({
        "runtime": "deno",
        "entry": "index.ts"
    });
    fs::write(dir.join("flux.json"), serde_json::to_string_pretty(&flux_json)?)?;

    let pkg_json = serde_json::json!({
        "name": name,
        "version": "0.1.0",
        "private": true,
        "type": "module",
        "dependencies": {
            "@fluxbase/functions": "*",
            "zod": "^3.23.0"
        }
    });
    fs::write(dir.join("package.json"), serde_json::to_string_pretty(&pkg_json)?)?;

    let index_ts = format!(
        r#"import {{ defineFunction }} from "@fluxbase/functions"
import {{ z }} from "zod"

const Input = z.object({{
  name: z.string(),
}})

const Output = z.object({{
  message: z.string(),
}})

export default defineFunction({{
  name: "{name}",
  description: "A simple hello-world function",
  input: Input,
  output: Output,
  handler: async ({{ input, ctx }}) => {{
    ctx.log("Executing {name}")
    return {{ message: `Hello ${{input.name}}` }}
  }},
}})
"#
    );
    fs::write(dir.join("index.ts"), index_ts)?;
    Ok(())
}

// ── Rust (WASM) ───────────────────────────────────────────────────────────────

fn scaffold_rust(dir: &Path, name: &str) -> anyhow::Result<()> {
    let flux_json = serde_json::json!({
        "runtime": "wasm",
        "entry":   "handler.wasm",
        "build":   "cargo build --target wasm32-wasip1 --release && cp target/wasm32-wasip1/release/handler.wasm handler.wasm"
    });
    fs::write(dir.join("flux.json"), serde_json::to_string_pretty(&flux_json)?)?;

    let cargo_toml = format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]
name = "handler"

[dependencies]
fluxbase-wasm-sdk = {{ path = "../../packages/wasm-sdk/rust" }}
serde        = {{ version = "1", features = ["derive"] }}
serde_json   = "1"
"#
    );
    fs::write(dir.join("Cargo.toml"), cargo_toml)?;

    fs::create_dir_all(dir.join("src"))?;
    let lib_rs = format!(
        r#"use fluxbase_wasm_sdk::prelude::*;

#[derive(serde::Deserialize)]
struct Input {{
    name: String,
}}

#[derive(serde::Serialize)]
struct Output {{
    message: String,
}}

fn handler(ctx: FluxCtx, input: Input) -> Result<Output, String> {{
    ctx.log(&format!("Executing {name}"));
    Ok(Output {{
        message: format!("Hello {{}}!", input.name),
    }})
}}

register_handler!(handler);
"#
    );
    fs::write(dir.join("src").join("lib.rs"), lib_rs)?;
    Ok(())
}

// ── Go (TinyGo) ───────────────────────────────────────────────────────────────

fn scaffold_go(dir: &Path, name: &str) -> anyhow::Result<()> {
    let flux_json = serde_json::json!({
        "runtime": "wasm",
        "entry":   "handler.wasm",
        "build":   "tinygo build -o handler.wasm -target wasip1 -scheduler none -no-debug ."
    });
    fs::write(dir.join("flux.json"), serde_json::to_string_pretty(&flux_json)?)?;

    let safe_name = name.replace('-', "_");
    let go_mod = format!(
        r#"module {safe_name}

go 1.21
"#
    );
    fs::write(dir.join("go.mod"), go_mod)?;

    let main_go = format!(
        r#"package main

import (
	"encoding/json"

	fluxbase "github.com/fluxbase/wasm-sdk-go"
)

type Input struct {{
	Name string `json:"name"`
}}

type Output struct {{
	Message string `json:"message"`
}}

func handler(ctx *fluxbase.Ctx, input Input) (Output, error) {{
	ctx.Log("{name}: executing")
	return Output{{Message: "Hello " + input.Name + "!"}}, nil
}}

func main() {{
	fluxbase.Register(func(ctx *fluxbase.Ctx, payload []byte) (any, error) {{
		var inp Input
		if err := json.Unmarshal(payload, &inp); err != nil {{
			return nil, err
		}}
		return handler(ctx, inp)
	}})
}}
"#
    );
    fs::write(dir.join("main.go"), main_go)?;
    Ok(())
}

// ── AssemblyScript ────────────────────────────────────────────────────────────

fn scaffold_assemblyscript(dir: &Path, name: &str) -> anyhow::Result<()> {
    let flux_json = serde_json::json!({
        "runtime": "wasm",
        "entry":   "build/handler.wasm",
        "build":   "npx asc assembly/index.ts --target release --outFile build/handler.wasm --exportRuntime"
    });
    fs::write(dir.join("flux.json"), serde_json::to_string_pretty(&flux_json)?)?;

    let pkg_json = serde_json::json!({
        "name": name,
        "version": "0.1.0",
        "private": true,
        "scripts": {
            "build": "asc assembly/index.ts --target release --outFile build/handler.wasm --exportRuntime"
        },
        "devDependencies": {
            "assemblyscript": "^0.27.0"
        }
    });
    fs::write(dir.join("package.json"), serde_json::to_string_pretty(&pkg_json)?)?;

    fs::create_dir_all(dir.join("assembly"))?;
    fs::create_dir_all(dir.join("build"))?;

    let asconfig = r#"{"targets":{"release":{"optimize":true,"noAssert":true,"outFile":"build/handler.wasm"}}}"#;
    fs::write(dir.join("asconfig.json"), asconfig)?;

    let index_ts = format!(
        r#"// === Fluxbase host imports ===
@external("fluxbase", "log")
declare function __flux_log(level: i32, ptr: i32, len: i32): void;

@external("fluxbase", "secrets_get")
declare function __flux_secrets_get(keyPtr: i32, keyLen: i32, outPtr: i32, outMax: i32): i32;

@external("fluxbase", "http_fetch")
declare function __flux_http_fetch(reqPtr: i32, reqLen: i32, outPtr: i32, outMax: i32): i32;

// === Minimal ABI helpers ===

export function __flux_alloc(size: i32): i32 {{
  return heap.alloc(size) as i32;
}}

function writeResult(json: string): i32 {{
  const encoded = String.UTF8.encode(json);
  const len = encoded.byteLength;
  const ptr = heap.alloc(4 + len) as i32;
  store<u32>(ptr, len as u32);
  memory.copy(ptr + 4, changetype<i32>(encoded), len);
  return ptr;
}}

// === Handler ===

export function handle(payloadPtr: i32, payloadLen: i32): i32 {{
  const payload = String.UTF8.decodeUnsafe(payloadPtr, payloadLen);

  // Parse input — expects {{ "name": "..." }}
  // (For production use a proper JSON parser library)
  let name = "world";
  const nameMatch = payload.indexOf('"name"');
  if (nameMatch >= 0) {{
    const colonIdx = payload.indexOf(':', nameMatch);
    const quoteStart = payload.indexOf('"', colonIdx + 1);
    const quoteEnd   = payload.indexOf('"', quoteStart + 1);
    if (quoteStart >= 0 && quoteEnd > quoteStart) {{
      name = payload.slice(quoteStart + 1, quoteEnd);
    }}
  }}

  const logMsg = "{name}: executing";
  const logBytes = String.UTF8.encode(logMsg);
  __flux_log(1, changetype<i32>(logBytes), logBytes.byteLength);

  const result = `{{"output":{{"message":"Hello ${{name}}!"}}}}`;
  return writeResult(result);
}}
"#
    );
    fs::write(dir.join("assembly").join("index.ts"), index_ts)?;
    Ok(())
}

// ── C ─────────────────────────────────────────────────────────────────────────

fn scaffold_c(dir: &Path, name: &str) -> anyhow::Result<()> {
    let flux_json = serde_json::json!({
        "runtime": "wasm",
        "entry":   "handler.wasm",
        "build":   "make"
    });
    fs::write(dir.join("flux.json"), serde_json::to_string_pretty(&flux_json)?)?;

    let makefile = format!(
        r#"CC      := /opt/wasi-sdk/bin/clang
CFLAGS  := --sysroot=/opt/wasi-sdk/share/wasi-sysroot -O2 -mexport-all
TARGET  := handler.wasm

.PHONY: all clean

all: $(TARGET)

$(TARGET): handler.c
	$(CC) $(CFLAGS) -o $@ $<

clean:
	rm -f $(TARGET)
"#
    );
    fs::write(dir.join("Makefile"), makefile)?;

    let handler_c = format!(
        r#"#include <stdint.h>
#include <string.h>
#include <stdlib.h>

/* ── Fluxbase host imports ──────────────────────────────────────────────── */
__attribute__((import_module("fluxbase"), import_name("log")))
extern void __flux_log(int32_t level, const char *ptr, int32_t len);

__attribute__((import_module("fluxbase"), import_name("secrets_get")))
extern int32_t __flux_secrets_get(const char *key_ptr, int32_t key_len,
                                  char *out_ptr, int32_t out_max);

__attribute__((import_module("fluxbase"), import_name("http_fetch")))
extern int32_t __flux_http_fetch(const char *req_ptr, int32_t req_len,
                                 char *out_ptr, int32_t out_max);

/* ── ABI ────────────────────────────────────────────────────────────────── */

__attribute__((export_name("__flux_alloc")))
void *flux_alloc(int32_t size) {{
    return malloc(size);
}}

/* Write [4-byte len][json] into a heap buffer and return its pointer */
static int32_t write_result(const char *json) {{
    int32_t len = (int32_t)strlen(json);
    char *buf = malloc(4 + len);
    /* little-endian u32 */
    buf[0] = (char)(len & 0xff);
    buf[1] = (char)((len >> 8) & 0xff);
    buf[2] = (char)((len >> 16) & 0xff);
    buf[3] = (char)((len >> 24) & 0xff);
    memcpy(buf + 4, json, len);
    return (int32_t)(uintptr_t)buf;
}}

/* ── Handler ────────────────────────────────────────────────────────────── */

__attribute__((export_name("handle")))
int32_t handle(int32_t payload_ptr, int32_t payload_len) {{
    char *payload = (char *)(uintptr_t)payload_ptr;

    /* Log invocation */
    const char *msg = "{name}: executing";
    __flux_log(1, msg, (int32_t)strlen(msg));

    /* Minimal JSON extraction — find "name" field value */
    char name_buf[128] = "world";
    char *p = strstr(payload, "\"name\"");
    if (p) {{
        p = strchr(p + 6, ':');
        if (p) {{
            p = strchr(p, '"');
            if (p) {{
                p++;
                int i = 0;
                while (*p && *p != '"' && i < 127)
                    name_buf[i++] = *p++;
                name_buf[i] = '\\0';
            }}
        }}
    }}

    /* Build result JSON */
    char result[256];
    snprintf(result, sizeof(result),
             "{{\"output\":{{\"message\":\"Hello %s!\"}}}}",
             name_buf);
    return write_result(result);
}}
"#
    );
    fs::write(dir.join("handler.c"), handler_c)?;
    Ok(())
}

// ── Zig ───────────────────────────────────────────────────────────────────────

fn scaffold_zig(dir: &Path, name: &str) -> anyhow::Result<()> {
    let flux_json = serde_json::json!({
        "runtime": "wasm",
        "entry":   "zig-out/bin/handler.wasm",
        "build":   "zig build -Dtarget=wasm32-wasip1 -Doptimize=ReleaseSmall"
    });
    fs::write(dir.join("flux.json"), serde_json::to_string_pretty(&flux_json)?)?;

    let build_zig = r#"const std = @import("std");

pub fn build(b: *std.Build) void {
    const target  = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const exe = b.addExecutable(.{
        .name       = "handler",
        .root_source_file = b.path("src/main.zig"),
        .target     = target,
        .optimize   = optimize,
    });

    b.installArtifact(exe);
}
"#;
    fs::write(dir.join("build.zig"), build_zig)?;

    fs::create_dir_all(dir.join("src"))?;
    let main_zig = format!(
        r#"const std = @import("std");

// ── Fluxbase host imports ─────────────────────────────────────────────────────
extern "fluxbase" fn log(level: i32, ptr: [*]const u8, len: i32) void;
extern "fluxbase" fn secrets_get(key_ptr: [*]const u8, key_len: i32,
                                  out_ptr: [*]u8, out_max: i32) i32;
extern "fluxbase" fn http_fetch(req_ptr: [*]const u8, req_len: i32,
                                 out_ptr: [*]u8, out_max: i32) i32;

// ── Allocator ─────────────────────────────────────────────────────────────────
var gpa = std.heap.GeneralPurposeAllocator(.{{}}){{}};
const allocator = gpa.allocator();

export fn __flux_alloc(size: i32) i32 {{
    const buf = allocator.alloc(u8, @intCast(size)) catch return 0;
    return @intCast(@intFromPtr(buf.ptr));
}}

// ── Handler ───────────────────────────────────────────────────────────────────
export fn handle(payload_ptr: i32, payload_len: i32) i32 {{
    const payload: []const u8 = @as([*]const u8, @ptrFromInt(@as(usize, @intCast(payload_ptr))))[0..@intCast(payload_len)];

    const msg = "{name}: executing";
    log(1, msg.ptr, msg.len);

    // Extract "name" field from payload (naive scan)
    var name_value: []const u8 = "world";
    if (std.mem.indexOf(u8, payload, "\"name\"")) |idx| {{
        const after = payload[idx + 6 ..];
        if (std.mem.indexOf(u8, after, "\"")) |q1| {{
            const rest = after[q1 + 1 ..];
            if (std.mem.indexOf(u8, rest, "\"")) |q2| {{
                name_value = rest[0..q2];
            }}
        }}
    }}

    const result = std.fmt.allocPrint(
        allocator,
        "{{{{\"output\":{{{{\"message\":\"Hello {{s}}!\"}}}}}}}}",
        .{{name_value}},
    ) catch return 0;

    const out = allocator.alloc(u8, 4 + result.len) catch return 0;
    const len: u32 = @intCast(result.len);
    out[0] = @intCast(len & 0xff);
    out[1] = @intCast((len >> 8) & 0xff);
    out[2] = @intCast((len >> 16) & 0xff);
    out[3] = @intCast((len >> 24) & 0xff);
    @memcpy(out[4..], result);
    return @intCast(@intFromPtr(out.ptr));
}}
"#
    );
    fs::write(dir.join("src").join("main.zig"), main_zig)?;
    Ok(())
}

// ── Python ────────────────────────────────────────────────────────────────────

fn scaffold_python(dir: &Path, name: &str) -> anyhow::Result<()> {
    let flux_json = serde_json::json!({
        "runtime": "wasm",
        "entry":   "handler.wasm",
        "build":   "py2wasm handler.py -o handler.wasm"
    });
    fs::write(dir.join("flux.json"), serde_json::to_string_pretty(&flux_json)?)?;

    let requirements = "py2wasm>=0.3.0\n";
    fs::write(dir.join("requirements.txt"), requirements)?;

    let handler_py = format!(
        r#"# {name} — Flux function written in Python (compiled via py2wasm)
# Build: py2wasm handler.py -o handler.wasm
#
# The Fluxbase WASM runtime calls `handle(payload_ptr, payload_len) -> result_ptr`.
# py2wasm exposes Python functions as WASM exports when decorated with @export.
#
# Note: Only a subset of Python is supported by py2wasm (no stdlib imports).

import json

# ── py2wasm export decorator (no-op at runtime, marks WASM export) ─────────────
def export(fn):
    return fn

# ── Memory helpers (provided by py2wasm runtime) ──────────────────────────────
# These stubs let the file parse cleanly; py2wasm replaces them at compile time.
def __alloc(size: int) -> int: ...
def __write_bytes(ptr: int, data: bytes) -> None: ...
def __read_bytes(ptr: int, length: int) -> bytes: ...

@export
def __flux_alloc(size: int) -> int:
    return __alloc(size)

@export
def handle(payload_ptr: int, payload_len: int) -> int:
    payload_bytes = __read_bytes(payload_ptr, payload_len)
    data = json.loads(payload_bytes.decode("utf-8"))

    name = data.get("name", "world")

    # Build result bytes: [4-byte LE length][JSON]
    result = json.dumps({{"output": {{"message": "Hello " + name + "!"}}}}).encode("utf-8")
    length  = len(result)
    buf_ptr = __alloc(4 + length)
    header  = bytearray([
        length & 0xff,
        (length >> 8)  & 0xff,
        (length >> 16) & 0xff,
        (length >> 24) & 0xff,
    ])
    __write_bytes(buf_ptr, bytes(header) + result)
    return buf_ptr
"#
    );
    fs::write(dir.join("handler.py"), handler_py)?;
    Ok(())
}

// ── Next-step hints ───────────────────────────────────────────────────────────

fn print_next_steps(name: &str, language: &Language) {
    match language {
        Language::Typescript => {
            println!("  cd {name}");
            println!("  npm install          # install @fluxbase/functions + zod");
            println!("  flux deploy          # bundle & deploy");
            println!("  flux invoke {name}   # test it");
        }
        Language::Rust => {
            println!("  cd {name}");
            println!("  # Requires: rustup target add wasm32-wasip1");
            println!("  flux deploy          # runs cargo build then uploads handler.wasm");
            println!("  flux invoke {name}   # test it");
        }
        Language::Go => {
            println!("  cd {name}");
            println!("  # Requires: https://tinygo.org/getting-started/install/");
            println!("  flux deploy          # runs tinygo build then uploads handler.wasm");
            println!("  flux invoke {name}   # test it");
        }
        Language::Assemblyscript => {
            println!("  cd {name}");
            println!("  npm install          # install assemblyscript compiler");
            println!("  flux deploy          # runs asc then uploads build/handler.wasm");
            println!("  flux invoke {name}   # test it");
        }
        Language::C => {
            println!("  cd {name}");
            println!("  # Requires: https://github.com/WebAssembly/wasi-sdk");
            println!("  #   brew install wasi-sdk  OR  download from GitHub releases");
            println!("  flux deploy          # runs make then uploads handler.wasm");
            println!("  flux invoke {name}   # test it");
        }
        Language::Zig => {
            println!("  cd {name}");
            println!("  # Requires: https://ziglang.org/download/");
            println!("  flux deploy          # runs zig build then uploads zig-out/bin/handler.wasm");
            println!("  flux invoke {name}   # test it");
        }
        Language::Python => {
            println!("  cd {name}");
            println!("  pip install py2wasm  # install the Python→WASM compiler");
            println!("  flux deploy          # runs py2wasm then uploads handler.wasm");
            println!("  flux invoke {name}   # test it");
        }
    }
}

// ── Languages table ───────────────────────────────────────────────────────────

fn print_languages() {
    println!("{:<16} {:<10} {:<50} INSTALL", "LANGUAGE", "RUNTIME", "TOOLCHAIN");
    println!("{}", "-".repeat(110));
    let rows: &[(&str, &str, &str, &str)] = &[
        ("typescript",     "deno",  "Node.js (for bundling)",                         "https://nodejs.org"),
        ("rust",           "wasm",  "rustup target add wasm32-wasip1",                "https://rustup.rs"),
        ("go",             "wasm",  "TinyGo (tinygo build)",                         "https://tinygo.org"),
        ("assemblyscript", "wasm",  "npm i -g assemblyscript",                        "https://www.assemblyscript.org"),
        ("c",              "wasm",  "wasi-sdk (clang --target=wasm32-wasi)",          "https://github.com/WebAssembly/wasi-sdk"),
        ("zig",            "wasm",  "zig build -Dtarget=wasm32-wasip1",              "https://ziglang.org"),
        ("python",         "wasm",  "py2wasm handler.py -o handler.wasm",            "https://github.com/astral-sh/py2wasm"),
    ];
    for (lang, rt, toolchain, url) in rows {
        println!("{:<16} {:<10} {:<50} {}", lang, rt, toolchain, url);
    }
}

// ── List deployed functions ───────────────────────────────────────────────────

async fn list_functions() -> anyhow::Result<()> {
    let client = ApiClient::new().await?;
    let res = client.client
        .get(format!("{}/functions", client.base_url))
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
