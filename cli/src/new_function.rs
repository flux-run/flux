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
  handler: async ({{ ctx, payload }}) => {{
    ctx.log("Running {name}");

    // ctx.db.<table>.find({{ where: ... }})   — query your database
    // ctx.secrets.MY_SECRET                   — read a secret
    // ctx.functions.<other>()                 — call another function

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
        r#"export default {{
  __fluxbase: true,

  /** @param {{{{payload: any, ctx: import("./.flux/ctx.js").FluxCtx}}}} args */
  async execute({{ payload, ctx }}) {{
    ctx.log("Running {name}");

    return {{ ok: true }};
  }},
}};
"#,
        name = name
    )
}

// ─── Rust ─────────────────────────────────────────────────────────────────────

fn scaffold_rust(name: &str) -> String {
    let pascal = pascal(name);
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
    format!(
        r#"//go:build wasip1

package main

import (
	"encoding/json"
	"fmt"
)

type Input struct {{
	// TODO: add your input fields
}}

type Output struct {{
	OK bool `json:"ok"`
}}

//export {name}_handler
func handler(inputPtr, inputLen uint32) uint64 {{
	inputBytes := readMemory(inputPtr, inputLen)

	var input Input
	if err := json.Unmarshal(inputBytes, &input); err != nil {{
		panic(fmt.Sprintf("{name}: unmarshal input: %v", err))
	}}

	out := Output{{OK: true}}
	outBytes, _ := json.Marshal(out)
	return writeMemory(outBytes)
}}

// Memory helpers (provided by the Flux WASM runtime).
func readMemory(ptr, length uint32) []byte {{
	return (*[1 << 30]byte)(unsafe.Pointer(uintptr(ptr)))[:length:length]
}}
func writeMemory(data []byte) uint64 {{
	ptr := uintptr(unsafe.Pointer(&data[0]))
	return (uint64(ptr) << 32) | uint64(len(data))
}}

func main() {{}}
"#,
        name = name
    )
}

// ─── Python ───────────────────────────────────────────────────────────────────

fn scaffold_python(name: &str) -> String {
    format!(
        r#"# {name} — Flux function (compiled to WASM via py2wasm)
# Build: py2wasm -i handler.py -o {name}.wasm

import json

def handler(input_json: str) -> str:
    \"\"\"Entry point called by the Flux runtime.\"\"\"
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
        r#"// {name} — Flux function (compiled to WASM via clang --target=wasm32)
// Build: clang --target=wasm32 -nostdlib -Wl,--no-entry -Wl,--export-all -o {name}.wasm handler.c
#include <stdint.h>
#include <string.h>

// Simple JSON response helper — replace with a proper JSON library.
static char output_buf[1024];

__attribute__((export_name("{name}_handler")))
uint64_t handler(uint8_t *input, uint32_t input_len) {{
    (void)input; (void)input_len;

    const char *resp = "{{\"ok\":true}}";
    uint32_t resp_len = (uint32_t)strlen(resp);
    memcpy(output_buf, resp, resp_len);

    return ((uint64_t)(uintptr_t)output_buf << 32) | resp_len;
}}
"#,
        name = name
    )
}

// ─── C++ ──────────────────────────────────────────────────────────────────────

fn scaffold_cpp(name: &str) -> String {
    format!(
        r#"// {name} — Flux function (compiled to WASM via clang++ --target=wasm32)
// Build: clang++ --target=wasm32 -nostdlib -Wl,--no-entry -Wl,--export-all -o {name}.wasm handler.cpp
#include <cstdint>
#include <cstring>

static char output_buf[1024];

extern "C" {{
    __attribute__((export_name("{name}_handler")))
    uint64_t handler(uint8_t* input, uint32_t input_len) {{
        (void)input; (void)input_len;

        const char* resp = "{{\"ok\":true}}";
        uint32_t resp_len = static_cast<uint32_t>(strlen(resp));
        memcpy(output_buf, resp, resp_len);

        return (static_cast<uint64_t>(reinterpret_cast<uintptr_t>(output_buf)) << 32) | resp_len;
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
import Foundation\n\
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
    let pascal = pascal(name);
    format!(
        r#"// {name} — Flux function (compiled to WASM via GraalVM Native Image + WASI)
// Build: native-image --no-fallback -H:Kind=SHARED_LIBRARY Handler.java
import com.oracle.svm.core.c.CTypedef;
import org.graalvm.nativeimage.c.function.CEntryPoint;
import java.nio.charset.*;

public class {pascal}Handler {{

    @CEntryPoint(name = "{name}_handler")
    public static long handle(org.graalvm.word.Pointer inputPtr, int inputLen) {{
        // TODO: decode input
        byte[] resp = "{{\"ok\":true}}".getBytes(StandardCharsets.UTF_8);
        // Return pointer + length packed into long.
        return ((long) resp.hashCode() << 32) | resp.length;
    }}
}}
"#,
        name = name,
        pascal = pascal
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
