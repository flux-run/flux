# WASM Runtime — Deferred

> **Status: Deferred.** WASM support is designed but not in scope for
> Phase 0–3. The framework currently supports Deno V8 (JavaScript/TypeScript)
> only. This document preserves the design for future implementation.

---

## Motivation

| Concern | Deno V8 only | With WASM |
|---|---|---|
| Language choice | JavaScript / TypeScript | Any WASM-capable language |
| Performance ceiling | JIT-compiled JS | AOT-compiled native code |
| Use cases | APIs, scripts, workflows | CPU-bound compute, ML inference |
| Existing code reuse | Rewrite required | Compile-and-deploy |

WASM does not replace Deno — it is a second execution target that sits
alongside it. The Runtime selects the engine at dispatch time based on the
`runtime` field in `flux.json`.

---

## Two-runtime model

```
POST /execute
       │
       ├─ runtime = "deno"  ──► IsolatePool (Deno V8, current)
       │
       └─ runtime = "wasm"  ──► WasmPool   (Wasmtime, future)
```

Both paths share: bundle fetching, secrets injection, structured logging,
request tracing, and resource limits.

---

## Design overview

### WasmPool

Mirrors `IsolatePool` but manages pre-instantiated Wasmtime `Store`s:

```
WasmPool {
  workers: min(2 × CPU, 16)
  cache:   compiled Module per function_id (AOT, Cranelift)
}
```

### FluxContext ABI

WASM functions access Flux capabilities via host imports:

```
flux.secret_get(key_ptr, key_len) → (val_ptr, val_len)
flux.log(level, msg_ptr, msg_len)
flux.http_fetch(req_ptr, req_len) → (resp_ptr, resp_len)
flux.db_query(req_ptr, req_len) → (resp_ptr, resp_len)
```

Guest allocator: `flux_alloc(size) → ptr` / `flux_free(ptr, size)`.

### Language support

| Language | Toolchain | Status |
|---|---|---|
| Rust | `cargo build --target wasm32-wasip1` | Designed |
| Go | `tinygo build -target wasm` | Designed |
| C/C++ | `clang --target=wasm32-wasi` | Designed |
| AssemblyScript | `asc` compiler | Designed |
| Python | `componentize-py` | Experimental |
| Zig | `zig build -target wasm32-wasi` | Designed |

### flux.json for WASM functions

```json
{
  "runtime": "wasm",
  "entry": "handler.wasm",
  "build": "cargo build --target wasm32-wasip1 --release && cp target/wasm32-wasip1/release/my_fn.wasm handler.wasm",
  "memory_mb": 64
}
```

---

## Security model

- Linear memory isolation per invocation
- No filesystem access (pure WASI subset)
- Fuel-based CPU metering (configurable per function)
- Memory limit enforced at instantiation

---

## Implementation plan

This will be implemented after Phase 3 (production readiness). Priority:
1. Rust WASM functions (most demand)
2. Go WASM functions
3. Other languages

The Runtime already has extension points in `runtime/src/engine/` for a
`WasmPool` module. See the source code for the existing `IsolatePool`
pattern to follow.

---

*For the current runtime (Deno V8), see [runtime.md](runtime.md).
For the overall architecture, see [framework.md §4](framework.md#4-architecture).*
