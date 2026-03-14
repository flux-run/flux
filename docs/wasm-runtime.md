# WASM Runtime

Flux supports both JavaScript-first execution and WebAssembly for multi-language functions.

The WASM path matters because it opens the runtime to any compiled language while preserving a consistent execution and debugging model.

## Why WASM Matters

WebAssembly gives Flux:

- language diversity — Rust, Go, Java, Python, PHP, and AssemblyScript run alongside TypeScript
- portable function bundles
- tighter runtime control and sandboxing
- a uniform deployment artifact for non-JavaScript languages
- first-compile AOT caching so cold starts are fast on every deploy after the first

Flux uses one runtime, not a different execution model for every language.

## Product Stance

JavaScript is the first-class default with the richest SDK. WASM extends the runtime to additional backend languages.

WASM provides:

- a coherent packaging model
- predictable execution
- visibility into spans, errors, and side effects
- deployment metadata that fits the same debugging story

## What Parity Means

Non-JavaScript functions participate in:

- deployment versioning
- trace generation
- mutation attribution
- queue and schedule execution
- replay and diff

Without that, WASM would be just an alternate build target, not part of the product.

## Packaging Model

The model is:

- source language compiles to a WASM artifact
- Flux stores that artifact as a function version
- the runtime loads and executes it under the same execution record model

The packaging process differs by language, but the operator experience stays consistent.

## Supported Languages

| Language | Toolchain | Warm p50 | Notes |
|----------|-----------|----------|-------|
| TypeScript / JS | Native Deno V8 | < 1 ms | First-class, full SDK |
| AssemblyScript | `asc` → wasm32-wasi | < 1 ms | TypeScript-like syntax, smallest binaries |
| Rust | `cargo` → wasm32-wasip1 | 1 ms | Best WASM toolchain, sub-100 KB binaries |
| Java | TeaVM → wasm32-wasi | 1 ms | No JVM warmup; compact 192 KB output |
| Go | `go build` GOOS=wasip1 | 19 ms | Standard toolchain, no TinyGo required |
| PHP | php-8.2-wasm (vmware-labs) | 83 ms | Full PHP 8.2, argv-embedded script |
| Python | py2wasm (Nuitka) | 191 ms | Compiled — not interpreter-in-WASM |
| C# (.NET) | dotnet `wasi-experimental` | — | 🚧 Coming soon — WASIP2 component model |
| Ruby | rbwasm | — | ❌ Compilation timeout — not viable |

## Constraints

WASM support comes with realistic constraints:

- some language features require adapters or tooling
- I/O and host bindings follow a stable contract
- performance and startup characteristics differ by language

Those constraints are acceptable because the runtime story stays coherent.

## Product Rule

WASM is valuable because it strengthens the complete-system story.

Language breadth does not weaken the debugging and deployment model.
