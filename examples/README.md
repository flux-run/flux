# Flux Examples

Hello-world examples for every language Flux supports.
Each directory is an independent Flux project — clone any one, run `flux dev`, and you're live.

| Example | Language | Runtime |
|---------|----------|---------|
| [hello-typescript](./hello-typescript) | TypeScript | Deno / Node |
| [hello-javascript](./hello-javascript) | JavaScript | Deno / Node |
| [hello-rust](./hello-rust) | Rust | WASM (Wasmtime) |
| [hello-go](./hello-go) | Go | WASM (TinyGo) |
| [hello-python](./hello-python) | Python | WASM (CPython port) |
| [hello-c](./hello-c) | C | WASM (wasi-sdk / clang) |
| [hello-cpp](./hello-cpp) | C++ | WASM (wasi-sdk / clang) |
| [hello-zig](./hello-zig) | Zig | WASM (native target) |
| [hello-assemblyscript](./hello-assemblyscript) | AssemblyScript | WASM (asc) |
| [hello-csharp](./hello-csharp) | C# | WASM (.NET WASI) |
| [hello-swift](./hello-swift) | Swift | WASM (swiftwasm) |
| [hello-kotlin](./hello-kotlin) | Kotlin | WASM (Kotlin/WASM) |
| [hello-java](./hello-java) | Java | WASM (GraalVM Native) |
| [hello-ruby](./hello-ruby) | Ruby | WASM (ruby.wasm) |

## Quick start (any example)

```bash
cd hello-typescript     # or any other language
flux dev                # starts local server + embedded Postgres (zero config)

# in another terminal
flux deploy
flux invoke hello
# → { "ok": true }
```

## How these were created

Each project was scaffolded with the Flux CLI:

```bash
flux init --name hello-<lang>
flux function create hello --language <lang>
```
