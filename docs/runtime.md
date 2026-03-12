# Runtime Service

> **Internal architecture doc.** This describes the Runtime service implementation
> for contributors. For user-facing docs, see [framework.md](framework.md).

---

## Overview

| Property | Value |
|---|---|
| Service name | `flux-runtime` |
| Role | Function execution engine |
| Tech | Rust, Axum, `deno_core` (V8 isolates), Wasmtime |
| Default port | `:8083` |
| Exposed to internet | No — receives traffic from Gateway and Queue only |

The Runtime executes user functions inside sandboxed Deno V8 isolates (and Wasmtime
for WASM bundles). It handles bundle fetching, secrets injection, `ctx` object
construction, structured logging, and execution trace emission.

```
Gateway :8081          Queue :8084
     │  POST /execute       │  POST /execute
     └──────────┬───────────┘
                ▼
         Runtime :8083
              ├── BundleCache  (function-level TTL 60s + deployment-level LRU)
              ├── SchemaCache  (input/output JSON Schema per function)
              ├── SecretsClient (LRU cache)
              ├── IsolatePool  (bounded channel, backpressure, V8 isolates)
              │       └── Worker thread N
              │              ├── JsRuntime (V8 isolate)
              │              └── FluxContext sandbox
              └── WasmPool    (compiled-module cache, Wasmtime)
```

---

## Execution flow

1. **Receive** `POST /execute` from Gateway or Queue — includes `function_id`, `project_id`, request body
2. **Check warm caches** — WasmPool bytes cache → BundleCache (function and deployment level)
3. **Cold fetch** — `GET /internal/bundle?function_id=...` from API service → R2/S3 presigned URL
4. **Fetch secrets** — SecretsClient with LRU cache
5. **Validate schema** — optional JSON Schema check on the input payload
6. **Execute** — dispatch to IsolatePool (Deno) or WasmPool (WASM) with timeout enforcement
7. **Emit trace** — spans + `ctx.log()` lines → API service `/internal/logs`
8. **Return result** — JSON response to caller

---

## IsolatePool

The isolate pool manages a fixed number of V8 worker threads:

```
IsolatePool {
  workers: min(2 × CPU, 16)
  channel: bounded tokio::mpsc (capacity = workers × 2)
}
```

- Each worker runs an independent `deno_core::JsRuntime`
- Isolates are reused across invocations
- If all workers are busy, requests queue in the channel
- If the channel is full, requests are rejected with 503

---

## FluxContext sandbox

The Runtime constructs a `ctx` object for each invocation:

| ctx property | Runtime implementation |
|---|---|
| `ctx.payload` | The inbound payload passed directly |
| `ctx.env` / `ctx.secrets.get()` | In-memory from SecretsClient cache |
| `ctx.log(msg, level)` | Structured log → collected and shipped at end of execution |
| `ctx.workflow.run(steps)` | Sequential step runner with per-step spans |
| `ctx.workflow.parallel(steps)` | `Promise.allSettled`-based parallel runner |
| `ctx.agent.run(options)` | LLM-driven tool-calling loop (requires `FLUXBASE_LLM_KEY` secret) |

---

## Bundle caching

Two-level cache:

| Level | Key | TTL | Description |
|---|---|---|---|
| Function-level | `function_id` | 60s | Quick lookup for repeat invocations |
| Deployment-level | `deployment_id` | LRU eviction | Avoids re-downloading from R2/S3 |

On cache miss: `GET /internal/bundle?function_id=...` → API service → R2/S3.

---

## WASM support

Wasmtime-based WASM execution runs in parallel with the Deno pool.
The `WasmPool` compiles WASM modules once and caches them by `function_id`.
The runtime auto-detects whether to use Deno or WASM based on the bundle
`runtime` field returned by the API, or the `X-Function-Runtime` hint header
from the Gateway.

---

## Secrets

`SecretsClient` fetches secrets from the API service with an LRU cache.
Secrets are never logged, never included in execution records, never returned
in error messages.

---

## Queue integration (retry)

The Queue service dispatches jobs directly to `POST /execute` with the same
payload shape as the Gateway. Failed executions are retried with exponential
backoff (5s × 2^attempt), then moved to dead-letter after `max_attempts`.

---

## Resource limits

| Limit | Default |
|---|---|
| Execution timeout (Deno) | 30s |
| WASM fuel limit | 1 billion instructions ≈ a few hundred ms |
| Request body size | 1 MB (runtime-side) / 10 MB (gateway-side) |

---

## Configuration

| Env var | Default | Description |
|---|---|---|
| `PORT` | `8083` | HTTP listen port |
| `API_URL` | `http://localhost:8080` | API service for bundle fetch, secrets, log emission |
| `CONTROL_PLANE_URL` | — | Backward-compat alias for `API_URL` |
| `SERVICE_TOKEN` | — | Service-to-service auth token |
| `ISOLATE_WORKERS` | `min(2 × CPU, 16)` | Number of V8 worker threads |

---

*Source: `runtime/src/`. For the full architecture, see
[framework.md §4](framework.md#4-architecture).*


---

## Overview

| Property | Value |
|---|---|
| Service name | `flux-runtime` |
| Role | Function execution engine |
| Tech | Rust, Axum, `deno_core` (V8 isolates) |
| Default port | `:8083` |
| Exposed to internet | No — receives traffic from Gateway only |

The Runtime executes user functions inside sandboxed Deno V8 isolates. It
handles bundle fetching, secrets injection, `ctx` object construction,
structured logging, and execution trace emission.

```
Gateway :8081
     │  POST /execute
     ▼
Runtime :8083
     ├── BundleCache (function-level, TTL 60s)
     ├── SecretsClient (LRU cache, 30s TTL)
     ├── IsolatePool (bounded channel, backpressure)
     │       └── Worker thread N
     │              ├── JsRuntime (V8 isolate)
     │              ├── FluxContext sandbox
     │              └── ToolExecutor (external APIs)
     │
     └── Execution record emitted to Data Engine
```

---

## Execution flow

1. **Receive** `POST /execute` from Gateway — includes `function_id`, `request_id`, request body
2. **Fetch bundle** — check BundleCache → API service → R2/S3 presigned URL
3. **Fetch secrets** — SecretsClient with LRU cache (30s TTL)
4. **Acquire isolate** — bounded channel with backpressure (rejects with 503 if full)
5. **Construct `ctx`** — build FluxContext with db proxy, queue client, secrets, logger
6. **Execute** — run user function in V8 isolate with timeout enforcement
7. **Emit execution record** — spans + mutations + calls → Data Engine
8. **Return result** — JSON response to Gateway

---

## IsolatePool

The isolate pool manages a fixed number of V8 worker threads:

```
IsolatePool {
  workers: min(2 × CPU, 16)
  channel: bounded tokio::mpsc (capacity = workers × 2)
}
```

- Each worker runs an independent `deno_core::JsRuntime`
- Isolates are reused across invocations (same function: cache hit, different function: reload)
- If all workers are busy, requests queue in the channel
- If the channel is full, requests are rejected with 503

---

## FluxContext sandbox

The Runtime constructs a `ctx` object for each invocation that maps to the
FluxContext interface defined in [framework.md §8](framework.md#8-functions--the-ctx-object):

| ctx property | Runtime implementation |
|---|---|
| `ctx.db.*` | HTTP calls to Data Engine `:8082` |
| `ctx.queue.push()` | HTTP call to Queue `:8084` |
| `ctx.function.invoke()` | HTTP call through Gateway with same `x-request-id` |
| `ctx.secrets.get()` | In-memory from SecretsClient cache |
| `ctx.tools.*` | ToolExecutor — external API calls |
| `ctx.log.*` | Structured log → execution span |
| `ctx.error()` | Throw structured error, terminate execution |

All `ctx.db` calls go through the Data Engine, which captures before/after state.
This is how `flux why` sees database mutations without the user doing anything.

---

## Bundle caching

Two-level cache:

| Level | Key | TTL | Description |
|---|---|---|---|
| Function-level | `function_id` | 60s | Quick lookup for repeat invocations |
| Deployment-level | `deployment_id` | Until redeploy | Compiled bundle |

On cache miss: `GET /internal/bundle?function_id=...` → API service → R2/S3.

---

## Secrets

`SecretsClient` fetches secrets from the API service with an LRU cache (30s TTL).
Secrets are never logged, never included in execution records, never returned
in error messages.

---

## Resource limits

| Limit | Default | Configurable via |
|---|---|---|
| Execution timeout | 30s | `flux.toml [limits].timeout_ms` / `flux.json` / `defineFunction()` |
| Memory | 128MB | `flux.toml [limits].memory_mb` / `flux.json` |
| Request body size | 10MB | Gateway `MAX_REQUEST_SIZE_BYTES` |

Precedence: `defineFunction()` > `flux.json` > `flux.toml [limits]`.

---

## Configuration

| Env var | Default | Description |
|---|---|---|
| `PORT` | `8083` | HTTP listen port |
| `CONTROL_PLANE_URL` | `http://localhost:8080` | API service for bundle + secrets |
| `DATA_ENGINE_URL` | `http://localhost:8082` | Data Engine for DB operations |
| `QUEUE_URL` | `http://localhost:8084` | Queue service |
| `GATEWAY_URL` | `http://localhost:8081` | For `ctx.function.invoke()` |
| `DATABASE_URL` | — | Postgres for execution record writes |
| `INTERNAL_SERVICE_TOKEN` | — | Service-to-service auth |
| `ISOLATE_POOL_SIZE` | `min(2 × CPU, 16)` | Number of V8 workers |
| `BUNDLE_CACHE_TTL_SECS` | `60` | Bundle cache TTL |
| `SECRETS_CACHE_TTL_SECS` | `30` | Secrets LRU cache TTL |

---

## WASM support (deferred)

The framework.md spec covers Deno V8 only. WASM support (Wasmtime) is designed
but deferred to Phase 4+. The Runtime architecture has extension points for a
`WasmPool` that mirrors `IsolatePool`. See the backup docs for the original
WASM design.

---

*Source: `runtime/src/`. For the full architecture, see
[framework.md §4](framework.md#4-architecture).*
