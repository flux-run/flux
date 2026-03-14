# WASM Runtime

Flux supports both JavaScript-first execution and WebAssembly for multi-language functions.

The WASM path matters because it opens the runtime to any compiled language while preserving a consistent execution and debugging model.

## Why WASM Matters

WebAssembly gives Flux:

- language diversity — Python, Go, Java, PHP, Rust, C#, and Ruby run alongside TypeScript
- portable function bundles
- tighter runtime control and sandboxing
- a uniform deployment artifact for non-JavaScript languages
- performance improvements for interpreted languages (PHP, Python) via AOT compilation

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

| Language | Toolchain | Notes |
|----------|-----------|-------|
| TypeScript | Native Deno V8 | First-class, full SDK |
| Python | WASM (Pyodide) | AOT compilation, faster than CPython cold starts |
| Go | WASM (TinyGo) | Smaller binaries than standard Go |
| Java | WASM (TeaVM/GraalVM) | No JVM warmup overhead |
| PHP | WASM (Emscripten) | Faster than PHP-FPM, underserved audience |
| Rust | WASM (native wasm32-wasi) | Best WASM toolchain, smallest bundles |
| C# | WASM (dotnet-wasi-sdk) | Full .NET without CLR overhead |
| Ruby | WASM (ruby.wasm) | Ruby core team maintained |

## Constraints

WASM support comes with realistic constraints:

- some language features require adapters or tooling
- I/O and host bindings follow a stable contract
- performance and startup characteristics differ by language

Those constraints are acceptable because the runtime story stays coherent.

## Product Rule

WASM is valuable because it strengthens the complete-system story.

Language breadth does not weaken the debugging and deployment model.
