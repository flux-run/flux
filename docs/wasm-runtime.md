# WASM Runtime — Design & Implementation Plan

Flux functions today run exclusively inside a Deno V8 isolate. This document
plans first-class **WebAssembly module support**, making Flux language-independent:
any language that compiles to WASM can be deployed as a Flux function.

---

## Table of Contents

1. [Motivation](#motivation)
2. [Two-Runtime Model](#two-runtime-model)
3. [WASM Execution Architecture](#wasm-execution-architecture)
4. [FluxContext ABI](#fluxcontext-abi)
5. [Language Support Matrix](#language-support-matrix)
6. [flux.json for WASM functions](#fluxjson-for-wasm-functions)
7. [Deployment Flow](#deployment-flow)
8. [WasmPool — Module Lifecycle](#wasmpool--module-lifecycle)
9. [Security Model](#security-model)
10. [Resource Limits](#resource-limits)
11. [Roadmap](#roadmap)

---

## Motivation

| Concern | Deno V8 only | With WASM |
|---|---|---|
| Language choice | JavaScript / TypeScript | Any WASM-capable language |
| Ecosystem | npm | native language toolchains |
| Performance ceiling | JIT-compiled JS | AOT-compiled native code |
| Use cases | APIs, scripts, workflows | CPU-bound compute, ML inference, Rust services |
| Existing code reuse | Re-write required | Compile-and-deploy existing libraries |

WASM does not replace Deno — it is a second execution target that sits
alongside it.  The runtime selects the engine at dispatch time based on the
`runtime` field in `flux.json`.

---

## Two-Runtime Model

```
POST /execute
       │
       ▼
  read flux.json
       │
       ├─ runtime = "deno"  ──► IsolatePool (Deno V8, existing)
       │
       └─ runtime = "wasm"  ──► WasmPool   (Wasmtime, new)
                                     │
                                     ▼
                               WasmExecutor
                                     │
                                     ▼
                              { result, logs }
```

Both paths share:
- Bundle fetching (BundleCache / S3 / R2)
- Secrets injection (SecretsClient, LRU 30 s)
- Structured logging (to data-engine)
- Request tracing (x-request-id propagation)
- Resource limits (timeout, memory)

Only the execution engine differs.

---

## WASM Execution Architecture

```
Runtime service
  ├── engine/
  │   ├── pool.rs           (IsolatePool — Deno, existing)
  │   ├── executor.rs       (JsRuntime worker, existing)
  │   ├── wasm_pool.rs      (WasmPool — new)
  │   └── wasm_executor.rs  (WasmtimeExecutor — new)
  └── api/
      └── routes.rs         (dispatches based on BundleMeta.runtime)
```

### WasmPool

Mirrors `IsolatePool` but manages a pool of **pre-instantiated Wasmtime
`Store`s** (one per worker slot).  Because WASM modules are sandboxed linear
memory, a `Store` can be safely reused across invocations of the same module by
resetting the memory backing between calls.

```
WasmPool {
  workers: min(2 × CPU, 16)
  cache:   compiled Module per function_id (AOT, Wasmtime Cranelift)
}
```

### Module compilation cache

First invocation:
1. Fetch `.wasm` bytes from BundleCache (same as JS bundles)
2. `wasmtime::Module::from_binary()` — Cranelift AOT → native code
3. Store compiled `Module` in an `LruCache<function_id, Arc<Module>>`

Subsequent invocations on the same `function_id`: skip steps 1–3, directly
instantiate from cached `Module` in ~1 ms.

### Per-invocation lifecycle

```
1. acquire Store from pool
2. instantiate Module (link host imports)
3. call export `handle(payload_ptr, payload_len) → (result_ptr, result_len)`
4. read result bytes from linear memory
5. reset memory & call stack
6. return Store to pool
```

---

## FluxContext ABI

The ABI is the contract between the WASM module and the Flux host.  It is
intentionally minimal — JSON in, JSON out — so that any language can implement it.

### Host imports (`fluxbase` namespace)

The host provides these imports to every WASM module:

| Import | Signature | Description |
|---|---|---|
| `fluxbase.secrets_get` | `(key_ptr, key_len, out_ptr, out_max) → i32` | Read a secret by name; returns bytes written or -1 |
| `fluxbase.log` | `(level: i32, msg_ptr, msg_len) → ()` | Emit a structured log line (0=debug … 3=error) |
| `fluxbase.http_fetch` | `(req_ptr, req_len, out_ptr, out_max) → i32` | Outbound HTTP gated by `WASM_HTTP_ALLOWED_HOSTS`; req/resp are JSON (`{"method","url","headers","body":<b64>}`) |

Both pointer arguments point into the module's linear memory.  The host reads
from / writes to that memory region using the module's exported
`__flux_alloc(len) → ptr` and `__flux_free(ptr, len)` helpers.

### Module exports (required)

```
__flux_alloc(len: i32) → ptr: i32   -- host asks module to allocate `len` bytes
__flux_free(ptr: i32, len: i32)     -- host releases allocation
handle(payload_ptr: i32, payload_len: i32) → result_ptr: i32
                                    -- main entry point; returns ptr to a
                                       length-prefixed JSON result byte string
```

### Payload / result format

Identical to the Deno runtime — a JSON object:

```json
// payload passed in
{
  "input":   { … },
  "secrets": { "KEY": "VALUE" },
  "request_id": "uuid"
}

// result returned
{
  "output": { … },   // or
  "error":  "message"
}
```

Logs are emitted via `fluxbase.log` during execution, not in the return value.

### WIT interface (future — WASI 0.2 Component Model)

Once Wasmtime's Component Model support stabilises, the low-level pointer ABI
will be replaced by a typed WIT interface:

```wit
// fluxbase:context/types  (wit/fluxbase.wit)
package fluxbase:context;

interface types {
  record invocation {
    payload: string,
    request-id: string,
  }

  resource secrets {
    get: func(key: string) -> option<string>;
  }

  log: func(level: string, message: string);
}

world flux-function {
  import types;
  export handle: func(inv: types.invocation) -> result<string, string>;
}
```

Language toolchains with Component Model support (Rust via `wit-bindgen`,
Go via `wasm-tools`) auto-generate the ABI glue from this interface.

---

## Language Support Matrix

| Language | Toolchain | WASM target | Notes |
|---|---|---|---|
| **Rust** | `cargo` + `wasm32-wasip1` | ✅ First-class | Best-in-class WASM support |
| **Go** | TinyGo | ✅ Supported | Use TinyGo ≥ 0.30 for WASIP1 |
| **C / C++** | `clang` + `wasm32-wasi` | ✅ Supported | Emscripten also works |
| **AssemblyScript** | `asc` | ✅ Supported | TypeScript-like syntax, tiny binary |
| **Python** | py2wasm / Nuitka | 🧪 Experimental | Large binary (~8 MB), slow cold start |
| **Kotlin** | Kotlin/Wasm | 🧪 Experimental | Compose/Wasm only today |
| **Swift** | SwiftWasm | 🧪 Experimental | Partial stdlib support |
| **Java / Kotlin (JVM)** | GraalVM native | ❌ Not planned | Too large for edge isolates |

---

## `flux.json` for WASM Functions

```json
{
  "runtime": "wasm",
  "entry":   "handler.wasm",
  "build":   "cargo build --target wasm32-wasip1 --release && cp target/wasm32-wasip1/release/my_handler.wasm handler.wasm",
  "memory_mb": 64,
  "timeout_ms": 10000,
  "allow_http": ["https://api.openai.com"]
}
```

| Field | Default | Description |
|---|---|---|
| `runtime` | — | `"wasm"` selects the WASM executor |
| `entry` | `handler.wasm` | Path to the `.wasm` file (relative to project root) |
| `build` | — | Shell command run by `flux deploy` before upload (optional) |
| `memory_mb` | `64` | Linear memory cap |
| `timeout_ms` | `30000` | Per-invocation timeout |
| `allow_http` | `[]` | URL prefix allow-list for outbound HTTP hosts |

If `build` is present, `flux deploy` runs it before packaging, similar to how
`esbuild` is invoked for Deno bundles.

---

## Deployment Flow

```
flux deploy (in a WASM project directory)
      │
      ├─ read flux.json → runtime = "wasm"
      │
      ├─ run flux.json.build command (cargo / tinygo / asc …)
      │
      ├─ read entry file (handler.wasm)
      │
      ├─ validate: must be valid WASM (magic bytes 0x00 0x61 0x73 0x6D)
      │            must export: handle, __flux_alloc, __flux_free
      │
      ├─ upload bytes to R2/S3  (same presigned URL flow as JS bundles)
      │
      └─ POST /internal/deployments  { runtime: "wasm", bundle_url: "…" }
```

The CLI validation step catches missing exports early — before the first cold
start exposes the error to an end user.

---

## WasmPool — Module Lifecycle

```
WasmPool (new)
  ├── compiled_modules: LruCache<function_id, Arc<Module>>   // AOT bytecode, shared
  └── worker_slots:     mpsc::channel<WasmWorker>            // N = min(2×CPU, 16)

WasmWorker
  ├── engine: Arc<wasmtime::Engine>   // shared, one per process
  └── store:  wasmtime::Store         // per-worker; reset between invocations
```

**Memory reset strategy:** Wasmtime exposes `Store::call_hook` and selective
memory zeroing.  After each invocation the worker zeroes only pages that were
written (tracked via `Memory::grow` accounting) before returning to the pool.
This avoids the ~1 ms full-zero overhead on every request after the first.

**Tenant isolation:** Same policy as `IsolatePool` — if a task arrives for a
different `tenant_id` than the slot's last tenant, the `Store` is fully
recreated before execution (heap, globals, linear memory all fresh).

---

## Security Model

WASM provides **memory-safe, capability-based sandboxing** at the hardware
level:

| Property | Detail |
|---|---|
| **Linear memory isolation** | The module can only read/write its own linear memory slab |
| **No ambient authority** | File system, network, clocks are all unavailable unless explicitly imported |
| **Host-gated HTTP** | `fluxbase.http_fetch` validates the target URL against `allow_http` before forwarding |
| **No FFI** | WASM cannot call native code or shared libraries |
| **Deterministic by default** | No access to `Date.now`, `Math.random` unless host injects them |

WASM's sandbox is stronger than V8's in some dimensions (no prototype pollution,
no dynamic `eval`, linear memory bounds are hardware-enforced) and weaker in
others (a buggy WASM module can corrupt its own heap but cannot affect the host).

---

## Resource Limits

| Limit | Default | Enforced by |
|---|---|---|
| Execution timeout | 30 s | `tokio::time::timeout` + Wasmtime fuel/epoch interrupts |
| Linear memory | 64 MB (`memory_mb`) | `wasmtime::MemoryType::new(…, max_pages)` |
| Worker slots | `min(2×CPU, 16)` | `WasmPool` channel backpressure |
| Compiled module cache | 256 entries | `LruCache` eviction |
| Outbound HTTP hosts | Per-function allow-list | `fluxbase.http_fetch` host import guard |

---

## Roadmap

### Phase 1 — Core WASM executor (v0.1)

- [x] Add `wasmtime` dependency to `runtime/Cargo.toml`
- [x] Implement `WasmPool` + `WasmExecutor` in `runtime/src/engine/`
- [x] Implement host imports: `fluxbase.secrets_get`, `fluxbase.log`
- [x] Add `runtime` field to `BundleMeta` (api + runtime)
- [x] Route `POST /execute` to `WasmPool` when `bundle_meta.runtime == "wasm"`
- [x] Validate WASM exports on upload (`flux deploy`) — magic-bytes + export check
- [x] Write Rust handler SDK (`packages/wasm-sdk/rust/`)
- [x] Gateway: extract `tenant_router` module, forward `X-Function-Runtime` header
- [x] Runtime: use `X-Function-Runtime` hint to skip irrelevant warm-path cache lookup
- [ ] End-to-end test: Rust function → deploy → invoke

### Phase 2 — HTTP + Component Model (v0.2)

- [x] `fluxbase.http_fetch` host import with allow-list enforcement (`WASM_HTTP_ALLOWED_HOSTS`)
- [ ] Integrate Wasmtime Component Model (`wit-bindgen` generated ABI)
- [ ] Write WIT interface file (`wit/fluxbase.wit`)
- [ ] Go (TinyGo) SDK + example
- [ ] AssemblyScript SDK + example
- [ ] Dashboard: show `runtime: wasm` badge on function cards

### Phase 3 — Production hardening (v0.3)

- [ ] Memory reset optimisation (dirty-page tracking)
- [ ] AOT-compiled module persistence to disk cache (skip recompile on deploy)
- [ ] `flux deploy --build` flag to explicitly trigger build step
- [ ] WASM binary size optimisation guide (`wasm-opt`, `wasm-strip`)
- [ ] Observability: per-invocation WASM memory/fuel metrics
- [ ] Python (py2wasm) experimental support

---

## Example: Rust WASM Function

### Project structure

```
my-function/
├── flux.json
├── Cargo.toml
└── src/
    └── lib.rs
```

### `flux.json`

```json
{
  "runtime": "wasm",
  "entry":   "handler.wasm",
  "build":   "cargo build --target wasm32-wasip1 --release && wasm-opt -Oz target/wasm32-wasip1/release/my_function.wasm -o handler.wasm"
}
```

### `Cargo.toml`

```toml
[package]
name    = "my-function"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
flux-wasm-sdk = "0.1"   # provides the ctx! macro and ABI glue
serde         = { version = "1", features = ["derive"] }
serde_json    = "1"
```

### `src/lib.rs`

```rust
use flux_wasm_sdk::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
struct Input {
    name: String,
    count: u32,
}

#[derive(Serialize)]
struct Output {
    message: String,
    doubled: u32,
}

#[flux_handler]
fn handle(ctx: FluxCtx, input: Input) -> FluxResult<Output> {
    ctx.log(format!("called with name={}", input.name));

    let api_key = ctx.secrets.get("OPENAI_API_KEY");
    ctx.log(format!("api_key present={}", api_key.is_some()));

    Ok(Output {
        message: format!("Hello, {}!", input.name),
        doubled: input.count * 2,
    })
}
```

The `#[flux_handler]` macro generates the `handle`, `__flux_alloc`, and
`__flux_free` exports and the JSON serialisation glue automatically.

---

## Example: Go (TinyGo) WASM Function

### `flux.json`

```json
{
  "runtime": "wasm",
  "entry":   "handler.wasm",
  "build":   "tinygo build -o handler.wasm -target wasi ."
}
```

### `main.go`

```go
package main

import (
    flux "github.com/fluxbase/wasm-sdk-go"
    "encoding/json"
)

type Input struct {
    Name string `json:"name"`
}

type Output struct {
    Message string `json:"message"`
}

func init() {
    flux.Register(func(ctx flux.Ctx, payload []byte) ([]byte, error) {
        var input Input
        json.Unmarshal(payload, &input)

        ctx.Log("info", "called with name=" + input.name)

        out, _ := json.Marshal(Output{Message: "Hello, " + input.Name + "!"})
        return out, nil
    })
}

func main() {}
```
