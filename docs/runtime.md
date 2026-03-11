# Runtime Service

The **Runtime** is the JavaScript execution engine of Fluxbase. It receives function invocation requests, fetches the compiled JS bundle, runs it inside an isolated Deno V8 sandbox, and returns a structured result with logs.

---

## Table of Contents

1. [Architecture Overview](#architecture-overview)
2. [Module Structure](#module-structure)
3. [HTTP API](#http-api)
4. [Security Model](#security-model)
5. [Execution Flow](#execution-flow)
6. [IsolatePool — Worker Thread Model](#isolatepool--worker-thread-model)
7. [Resource Limits](#resource-limits)
8. [V8 Sandbox & FluxContext](#v8-sandbox--fluxcontext)
9. [Caching Layer](#caching-layer)
10. [Secrets Management](#secrets-management)
11. [Tools Layer](#tools-layer)
12. [Workflow Engine](#workflow-engine)
13. [Agent Engine](#agent-engine)
14. [Triggers](#triggers)
15. [Deterministic Execution Model](#deterministic-execution-model)
16. [Replay Execution Mode](#replay-execution-mode)
17. [Execution Comparison](#execution-comparison)
18. [Observability & Logging](#observability--logging)
18. [Runtime Instrumentation](#runtime-instrumentation)
19. [Performance Characteristics](#performance-characteristics)
20. [Configuration](#configuration)
21. [Dependencies](#dependencies)
22. [Known Limitations & Improvement Areas](#known-limitations--improvement-areas)

---

## Architecture Overview

### Service call chain

```
Client
  ↓
Gateway  (auth, rate-limit, route resolution)
  ↓  x-request-id, x-parent-span-id, X-Tenant-Id, X-Tenant-Slug
Runtime  (bundle fetch, secret inject, V8 execution)
  ↓
V8 Isolate  (user JS bundle, FluxContext sandbox)
  ↓
ToolExecutor  (single unified call path)
  ↓
External APIs  (Composio, LLM, …)
```

### Execution internals

```
Gateway / API
      │
      ▼
POST /execute
      │
      ├─ BundleCache hit? ──────────────────────┐
      │  (function-level, TTL 60s)              │
      │                                         │
      ▼                                         │
  Control Plane                                  │
  GET /internal/bundle?function_id=...          │
      │                                         │
      ├─ deployment-level cache hit? ───────────┤
      │                                         │
      ▼                                         │
  S3 / R2 presigned URL (or inline DB code)    │
      │                                         │
      ▼                                         │
  BundleCache.insert_both()                     │
      │◄────────────────────────────────────────┘
      │
      ▼
  SecretsClient.fetch_secrets()   (LRU cache, 30s TTL)
      │
      ▼
  IsolatePool.execute()    (bounded channel, backpressure)
      │
      ▼
  isolate-worker-N thread
      │
  execute_function()
      │
  Deno JsRuntime  ←──── op_execute_tool    ──►  Composio API
  (fresh per call)◄──── op_agent_llm_call  ──►  OpenAI-compatible LLM
      │
      ▼
  ExecutionResult { output, logs }
      │
      ├─ logs ──► fire-and-forget POST /internal/logs
      │
      ▼
  HTTP 200  { result, duration_ms }
```

---

## Module Structure

```
runtime/src/
├── main.rs                  # Entry point: HTTP server, AppState, route wiring
├── api/
│   ├── mod.rs
│   └── routes.rs            # execute_handler, health_check, invalidate_cache_handler
├── engine/
│   ├── mod.rs
│   ├── executor.rs          # execute_function(), Deno ops, FluxContext JS wrapper
│   └── pool.rs              # IsolatePool — fixed OS thread pool
├── cache/
│   ├── mod.rs
│   └── bundle_cache.rs      # Two-level LRU bundle cache (function + deployment)
├── secrets/
│   ├── mod.rs
│   └── secrets_client.rs    # SecretsClient + SecretsCache (LRU, 30s TTL)
├── tools/
│   ├── mod.rs
│   ├── composio.rs          # Composio REST adapter (the only Composio-aware file)
│   ├── executor.rs          # ToolExecutor — single execution path for all tools
│   └── registry.rs          # ToolRegistry — maps "app.action" → Composio action ID
├── workflow/
│   └── mod.rs               # WorkflowStepRecord type; engine is in JS sandbox
├── agent/
│   ├── mod.rs               # AgentOpState
│   └── llm.rs               # call_llm() — OpenAI-compatible chat completions client
├── triggers/
│   ├── mod.rs
│   ├── registry.rs          # TriggerRegistry — in-memory trigger bindings
│   └── router.rs            # TriggerRouter — routes raw events to resolved triggers
└── config/
    ├── mod.rs
    └── settings.rs          # Settings loaded from environment variables
```

---

## HTTP API

| Method | Path | Auth | Purpose |
|--------|------|------|---------|
| `GET` | `/health` | none | Liveness probe — returns `{"status":"ok"}` |
| `GET` | `/version` | none | Build metadata: commit SHA, build time, isolate_workers |
| `POST` | `/execute` | `X-Service-Token` (via gateway) | Execute a function |
| `POST` | `/internal/cache/invalidate` | `X-Service-Token` | Evict bundle / secret cache entries |

### POST /execute

**Request headers**

| Header | Purpose |
|--------|---------|
| `X-Tenant-Id` | UUID of the tenant (forwarded from Gateway) |
| `X-Tenant-Slug` | Human slug (forwarded from Gateway) |
| `x-request-id` | Trace propagation ID (forwarded from Gateway) |
| `x-parent-span-id` | Parent span for trace hierarchy (forwarded from Gateway) |

**Request body**

```json
{
  "function_id": "uuid",
  "tenant_id":   "uuid",
  "project_id":  "uuid or null",
  "payload":     { /* arbitrary JSON, passed as ctx.payload */ }
}
```

**Response (success)**

```json
{
  "result":      { /* return value of the function */ },
  "duration_ms": 142
}
```

**Error codes**

| Code | HTTP | Meaning |
|------|------|---------|
| `FunctionExecutionError` | 500 | Unhandled exception in user code |
| `INPUT_VALIDATION_ERROR` | 400 | Function's `defineFunction` schema rejected input |
| `OUTPUT_VALIDATION_ERROR` | 500 | Function's `defineFunction` schema rejected output |
| `BundleFetchError` | 502 | Could not reach control plane |
| `no_bundle_found` | 404 | No active deployment for this function |
| `S3FetchError` | 500 | Could not download bundle from R2/S3 |
| `SecretFetchError` | 500 | Could not fetch secrets from control plane |

### POST /internal/cache/invalidate

Called by the control plane after a new deployment goes live, so the runtime stops serving the old bundle immediately instead of waiting for the TTL.

```json
{
  "function_id":   "optional",
  "deployment_id": "optional",
  "tenant_id":     "optional uuid — evicts secrets cache",
  "project_id":    "optional uuid"
}
```

---

## Security Model

The Runtime is an **internal service**. It must never accept direct public traffic.

### Trust boundary

```
[Public Internet] → Gateway (auth, rate-limit) → Runtime (service-to-service only)
```

All production traffic must flow through the Gateway. The Runtime is deployed on Cloud Run with ingress restricted to `internal-and-cloud-load-balancing` and should not be publicly reachable.

### Authentication

Every request to `/execute` and `/internal/cache/invalidate` must carry:

| Mechanism | Where verified |
|-----------|---------------|
| `X-Service-Token` header | Checked against `SERVICE_TOKEN` env var by the handler |
| `X-Tenant-Id` header | Must be present; used for scoping secrets and logs |
| `X-Tenant-Slug` header | Must be present; injected into sandbox as `ctx.tenant.slug` |

Requests missing the service token receive `HTTP 401`.

### What the Runtime does NOT enforce

The Gateway is responsible for:

- Firebase JWT verification
- Per-tenant rate limiting
- Function-level authorization (ownership check)
- Project-level isolation

The Runtime trusts these checks have already been performed by any caller that holds a valid `SERVICE_TOKEN`. **Never expose `SERVICE_TOKEN` outside the platform infrastructure.**

### Composio scoping

Each tenant's tool calls are isolated under their Composio entity ID (defaulting to `tenant_id`). A function from tenant A cannot access tenant B's connected accounts because tool calls include `entityId` in every Composio API request.

---

## Execution Flow

1. **Request received** — `execute_handler` extracts tenant ID, slug, request ID, and parent span ID from headers. The Gateway forwards: `X-Tenant-Id`, `X-Tenant-Slug`, `x-request-id`, `x-parent-span-id`.

2. **Bundle cache check (function-level)** — `BundleCache::get_by_function(function_id)`. Hit = 0 network calls to control plane.

3. **Secrets fetch** — `SecretsClient::fetch_secrets(tenant_id, project_id)`. Returns a `HashMap<String, String>` from the LRU cache or the control plane.

4. **Bundle fetch (on miss)** — `GET {CONTROL_PLANE_URL}/internal/bundle?function_id=...`
   - Response contains `deployment_id` + either a presigned `url` (R2/S3) or inline `code`.
   - Checks deployment-level cache first to avoid repeat S3 downloads.
   - On successful download: `BundleCache::insert_both(function_id, deployment_id, code)` warms both cache levels.

5. **Dispatch to IsolatePool** — `IsolatePool::execute(code, secrets, payload, tenant_id, tenant_slug)`.
   - Sends `ExecutionTask` over an async `mpsc` channel.
   - Awaits reply on a `oneshot` channel with an 11-second outer timeout.

6. **Worker executes** — one of N isolate-worker threads picks up the task and calls `execute_function()`.

7. **Logs forwarded** — fire-and-forget `tokio::spawn` posts each log line and a final `end` span to `{CONTROL_PLANE_URL}/internal/logs`.

8. **Response returned** — `{ result, duration_ms }`.

---

## IsolatePool — Worker Thread Model

**Problem without pooling**: every invocation would spawn a new OS thread (~0.5 ms, 8 MB stack), its own Tokio runtime (~1 ms), and a fresh `JsRuntime` (V8 heap + extension registration, ~3–5 ms). Under load this creates unbounded threads, memory pressure, and per-call startup overhead.

**Solution**: a fixed pool of pre-spawned OS threads, each owning a dedicated single-threaded Tokio runtime **and a single warm `JsRuntime` that persists across requests** (warm-isolate model). The per-request cost is reduced to an `OpState` swap (nanoseconds).

### Pool sizing

| Variable | Default | Description |
|----------|---------|-------------|
| `ISOLATE_WORKERS` | `min(2×CPU, 16)` | Number of worker threads |
| channel buffer | `workers × 4` | Burst capacity before backpressure kicks in |

### Why dedicated threads and not `tokio::spawn`?

Deno's `JsRuntime` is `!Send`. It must be created and used on the same thread. A regular `tokio::task` can be moved between threads by the scheduler. A dedicated OS thread guarantees the runtime stays pinned, which is also required for the warm-isolate model.

### Worker lifecycle

```
                 ┌──────────────────────────────────────────┐
                 │  isolate-worker-N (OS thread)            │
                 │                                          │
                 │  JsRuntime created ONCE at startup       │
                 │  tokio::runtime (single-thread)          │
                 │  ┌──────────────────────────────────┐    │
                 │  │  loop {                          │    │
                 │  │    task = rx.recv()              │    │
                 │  │    // hot path: OpState swap     │    │
                 │  │    execute_with_runtime(&mut rt) │    │
                 │  │    reply.send(result)            │    │
                 │  │    // on timeout: recreate rt    │    │
                 │  │    if timed_out { rt = create_js_runtime() }   │
                 │  │  }                               │    │
                 │  └──────────────────────────────────┘    │
                 └──────────────────────────────────────────┘
```

### JsRuntime warm-isolate safety

Each request updates the worker's `JsRuntime` `OpState` (secrets, tenant context) via `try_take` + `put` before execution. This is the only per-request mutation; the V8 heap, extension registrations, and snapshot are already in place.

`create_js_runtime()` applies two hardening steps **once at worker startup**, before any user code ever touches the isolate:

**1. Prototype freeze** — `Object.freeze` is applied to the most-abused built-in prototype objects, preventing user code from poisoning shared prototype chains across tenants:

```js
const __protos = [
    Object, Array, Function, String, Number, Boolean,
    RegExp, Promise, Map, Set, WeakMap, WeakSet, Error,
    TypeError, RangeError, SyntaxError, ReferenceError
];
for (const C of __protos) {
    if (C && C.prototype) Object.freeze(C.prototype);
}
```

Without this, a malicious or buggy bundle could do `Array.prototype.map = () => []` and break every subsequent invocation on the same worker. After the freeze, any attempt to mutate a frozen prototype throws a `TypeError` (in strict mode) or silently fails — it does not affect the shared prototype.

Cost: ~20 µs at worker startup; zero per-request overhead.

**2. Baseline global snapshot + per-invocation sweep** — immediately after the freeze, `create_js_runtime()` captures all current `globalThis` key names:

```js
globalThis.__fluxbase_allowed_globals =
    new Set(Object.getOwnPropertyNames(globalThis));
```

At the top of every IIFE wrapper, before the user bundle runs, any key not in that baseline set is deleted:

```js
if (typeof __fluxbase_allowed_globals !== "undefined") {
    for (const __k of Object.getOwnPropertyNames(globalThis)) {
        if (!__fluxbase_allowed_globals.has(__k)) {
            try { delete globalThis[__k]; } catch (_) {}
        }
    }
}
```

User code that writes `globalThis.cache = ...` or any custom global will have that value deleted before the next invocation on the same worker. Cost: O(n) over user-added keys only — typically zero in normal code.

**Isolation matrix after both guards:**

| Scope | Isolated? | Mechanism |
|-------|-----------|-----------|
| `__ctx`, `__payload`, `__secrets`, logs | ✅ per-request | IIFE closure |
| `globalThis.*` (user-set) | ✅ per-request | baseline sweep |
| Built-in prototypes | ✅ immutable | `Object.freeze` |
| V8 heap / JIT cache | shared (intentional) | warm isolate benefit |

If a request **times out**, the V8 event loop may be in an inconsistent state. The worker recreates its `JsRuntime` and clears the tenant affinity so the next task re-applies the full tenant check:

```rust
if matches!(&result, Err(e) if e.contains("timed out")) {
    js_rt = create_js_runtime();
    current_tenant = None;  // forces affinity re-check on next task
}
```

### Tenant affinity

Each worker thread tracks `current_tenant_id`. When a task arrives for a *different* tenant, the worker recreates its `JsRuntime` before executing. This ensures no V8 heap state, closure references, or `OpState` residue from tenant A can ever reach tenant B.

```rust
let tenant_changed = match &current_tenant {
    Some(prev) if prev != &task.tenant_id => true,
    _ => false,
};
if tenant_changed {
    js_rt = create_js_runtime();
}
current_tenant = Some(task.tenant_id.clone());
```

In practice tasks arrive in per-tenant bursts, so tenant switches are infrequent and the warm-isolate benefit is fully preserved within each tenant's burst.

### Deterministic replay (planned)

The current runtime is **not fully deterministic** across replay attempts because user code can observe:

- `Date.now()` / `new Date()` — wall clock time
- `Math.random()` — PRNG seeded per-isolate
- `performance.now()` — high-resolution monotonic timer

For `flux replay` and incident time-travel to produce identical results, these must be overridden with recorded values. The planned approach injects overrides into the IIFE wrapper using a `ReplayCtx` carried in `OpState`:

```js
// injected at replay time only
Date.now = () => {replay_epoch_ms};
Math.random = (() => {
    let _s = {replay_seed};
    return () => { _s = (_s * 1664525 + 1013904223) & 0xFFFFFFFF; return _s / 0xFFFFFFFF; };
})();
```

This is a non-breaking, per-request injection — live executions are unaffected. The recorded seed and epoch are stored alongside each execution's trace span, making replay 100% reproducible.

### Timeouts

| Boundary | Timeout | What it guards |
|----------|---------|---------------|
| outer (pool level) | 11 s | Time to acquire a worker |
| inner (V8 level) | 30 s | Function execution time |
| OS thread | 32 s | Backstop if inner timeout is bypassed |

---

## Resource Limits

These are the enforced and intended limits for every function invocation. They prevent abuse and protect other tenants sharing the same runtime instance.

### Enforced today

| Limit | Value | Enforced by |
|-------|-------|-------------|
| Max execution time | 30 s | `tokio::time::timeout` in `execute_function()` |
| Max request body | 1 MB | `DefaultBodyLimit::max(1 * 1024 * 1024)` in `main.rs` |
| Isolate worker slots | `min(2×CPU, 16)` | `IsolatePool` channel backpressure |
| Worker acquisition timeout | 11 s | outer timeout in `IsolatePool::execute()` |

### Intended (not yet enforced)

| Limit | Target | Notes |
|-------|--------|-------|
| Max result payload | 5 MB | Prevent large in-memory serializations |
| Max logs per invocation | 500 lines | Prevent log spam filling the database |
| Max tool calls per invocation | 50 | Prevent infinite tool loops |
| Max agent steps | 20 | Hard cap regardless of `maxSteps` option |
| Max memory per isolate | ~128 MB | V8 heap limit via `--max-heap-size` flag |

When execution time is exceeded, the function returns:

```json
{ "error": "FunctionExecutionError", "message": "Function execution timed out after 30 seconds" }
```

When the worker pool is saturated (all workers busy and the channel buffer full), new requests block at the `mpsc::send()` call. The 11-second outer timeout then fires, returning:

```json
{ "error": "FunctionExecutionError", "message": "isolate pool: invocation timed out waiting for worker" }
```

---

## V8 Sandbox & FluxContext

Each function execution runs inside a **warm `JsRuntime`** that is reused across calls on the same worker thread. Tenant secrets and payload are injected fresh on every request via `OpState`; there is no JavaScript state leakage between calls. The bundle code is executed inside a wrapper IIFE that injects the `__ctx` object.

### Bundle format

The runtime accepts two export styles:

```js
// Style 1: defineFunction() (schema-validated)
__fluxbase_fn = defineFunction({ schema: ... }, async (payload, ctx) => { ... });

// Style 2: plain async function
__fluxbase_fn = async (ctx) => { ... };
```

esbuild bundles wrap the default export under `.default`; the executor unwraps this automatically.

### FluxContext (`__ctx`)

Every function receives a `ctx` object with the following API:

#### `ctx.tenant`
```js
ctx.tenant.id   // UUID string
ctx.tenant.slug // human slug, e.g. "acme-org"
```

#### `ctx.payload`
The raw JSON payload from the invocation request.

#### `ctx.env` / `ctx.secrets`
```js
ctx.env["MY_KEY"]          // direct map access
ctx.secrets.get("MY_KEY")  // returns null if missing (no throw)
```

#### `ctx.log(message, level?)`
Structured logging. Level defaults to `"info"`. Logs are collected and batch-posted to the control plane after execution.

```js
ctx.log("processing item 42");
ctx.log("unexpected state", "warn");
```

#### `ctx.tools.run(toolName, input)`
Call an external integration. Emits a `tool` span visible in the trace viewer.

```js
const result = await ctx.tools.run("slack.send_message", {
  channel: "#ops",
  text: "Deploy complete",
});
```

Errors from tool calls are caught, logged as `error`-level spans, and re-thrown with a descriptive prefix (`tool:slack.send_message failed: ...`).

#### `ctx.workflow.run(steps, options?)`
Sequential multi-step orchestration. Each step receives `(ctx, previousOutputs)`.

```js
const outputs = await ctx.workflow.run([
  { name: "fetch_user",    fn: async (ctx, prev) => { ... } },
  { name: "send_welcome",  fn: async (ctx, prev) => { ... } },
], { continueOnError: false });

outputs.fetch_user    // result of step 1
outputs.send_welcome  // result of step 2
```

- `continueOnError: true` — failed steps store `{ __error: "..." }` instead of throwing.
- Each step emits a `workflow_step` span with name and duration.

#### `ctx.workflow.parallel(steps)`
Concurrent execution via `Promise.allSettled`. All steps start simultaneously.

```js
const outputs = await ctx.workflow.parallel([
  { name: "fetch_a", fn: async (ctx) => { ... } },
  { name: "fetch_b", fn: async (ctx) => { ... } },
]);
```

Failed steps are returned as `{ __error: "..." }` (never throws for individual step failures).

#### `ctx.agent.run(options)`
LLM-driven autonomous tool selection loop.

```js
const result = await ctx.agent.run({
  goal:     "Send a welcome email and create a Linear ticket",
  tools:    ["gmail.send_email", "linear.create_issue"],
  maxSteps: 5,
});
// result.answer  — LLM summary of what was done
// result.steps   — how many LLM calls were made
// result.output  — last tool output
```

- The agent loop runs entirely in JS: `agent.run()` → `op_agent_llm_call` → LLM decides action → `ctx.tools.run()` → next LLM call.
- Emits one `agent_step` span per LLM invocation.
- Throws `"agent: exceeded maxSteps"` if the goal is not achieved within the limit.

### Deno ops registered per execution

| Op | Direction | Purpose |
|----|-----------|---------|
| `op_execute_tool` | JS → Rust | Calls `ToolExecutor` → Composio REST API |
| `op_agent_llm_call` | JS → Rust | Calls OpenAI-compatible chat completions |

Both ops are registered on a custom `Extension` named `"fluxbase"`.

---

## Caching Layer

### Bundle cache — two levels

```
by_function    HashMap<function_id, (code, inserted_at)>   TTL: 60s
by_deployment  HashMap<deployment_id, code>                 No TTL (LRU eviction)
```

**Warm path** (function-level cache hit): 0 HTTP calls to control plane, 0 S3 calls.

**Tepid path** (deployment-level cache hit, function miss): re-warms function cache from deployment entry.

**Cold path**: control plane → S3/R2 download → inserts into both levels.

**Invalidation**: `POST /internal/cache/invalidate` with `X-Service-Token`. Called by the control plane on every new deployment. Evicts function and/or deployment entries immediately.

### Capacity

```
capacity = 100 entries per sub-cache (LRU eviction when full)
```

---

## Secrets Management

`SecretsClient` fetches all secrets for a `(tenant_id, project_id)` pair from:

```
GET {CONTROL_PLANE_URL}/internal/secrets?tenant_id=...&project_id=...
```

### Secrets cache

- Type: LRU, capacity 50 entries
- TTL: 30 seconds
- Cache key: `"<tenant_id>/<project_id>"` or `"<tenant_id>/none"`
- Invalidation: `POST /internal/cache/invalidate` with `tenant_id` evicts the entry immediately

Secrets are injected into the JS sandbox as both `ctx.env` (direct map) and `ctx.secrets.get()` (null-safe accessor).

### Reserved secret names

| Secret | Used by | Purpose |
|--------|---------|---------|
| `FLUXBASE_LLM_KEY` | Agent | LLM API key |
| `FLUXBASE_LLM_URL` | Agent | Override LLM endpoint (default: OpenAI) |
| `FLUXBASE_LLM_MODEL` | Agent | Override model (default: `gpt-4o-mini`) |

---

## Tools Layer

### Single execution rule

> All tool invocations flow through one path: `ctx.tools.run()` → `op_execute_tool` → `ToolExecutor.run()` → `composio::execute_action()`

Nothing bypasses `ToolExecutor`. This gives uniform trace visibility across functions, workflows, and agents.

### ToolRegistry

Maps Fluxbase developer-facing names to Composio action IDs:

```
"slack.send_message"  →  { composio_action: "SLACK_SEND_MESSAGE", app: "slack" }
```

The registry is the **only** place that knows about Composio action IDs. To add a new tool, add one line to the entries table in `registry.rs`.

### Registered tools (Phase 1)

| App | Fluxbase name | Description |
|-----|---------------|-------------|
| **Slack** | `slack.send_message` | Post to channel or DM |
| | `slack.create_channel` | Create a new channel |
| | `slack.invite_to_channel` | Invite users to a channel |
| **GitHub** | `github.create_issue` | Open a new issue |
| | `github.create_pr` | Open a pull request |
| | `github.add_comment` | Comment on issue or PR |
| | `github.list_issues` | List open issues |
| | `github.star_repo` | Star a repository |
| **Gmail** | `gmail.send_email` | Send an email |
| | `gmail.create_draft` | Create a draft |
| **Outlook** | `outlook.send_email` | Send via Outlook |
| | `outlook.create_draft` | Create a draft |
| | `outlook.reply_email` | Reply to an email |
| **Linear** | `linear.create_issue` | Create an issue |
| | `linear.update_issue` | Update an issue |
| **Notion** | `notion.create_page` | Create a page |
| | `notion.update_page` | Update page properties |
| **Jira** | `jira.create_issue` | Create an issue |
| | `jira.update_issue` | Update an issue |
| | `jira.add_comment` | Add a comment |
| **Airtable** | `airtable.create_record` | Create a record |
| | `airtable.list_records` | List records |
| **Google Sheets** | `sheets.append_row` | Append a row |
| **Stripe** | `stripe.create_customer` | Create a customer |
| | `stripe.create_invoice` | Create an invoice |

### Composio adapter

`composio.rs` is the **only** Composio-aware file. To swap the tool provider, replace this file.

```
POST https://backend.composio.dev/api/v2/actions/{ACTION_NAME}/execute
x-api-key: {COMPOSIO_API_KEY}        ← platform-level key, not user-supplied
Body: { "entityId": "<tenant_id>", "appName": "<app>", "input": { ... } }
```

Each tenant is a Composio **entity** identified by their `tenant_id` UUID. Their connected accounts (OAuth tokens) live under that entity, providing cross-tenant isolation.

**Override**: `COMPOSIO_ENTITY_ID` env var overrides the entity (used for shared demo accounts like `fluxbase-demo`).

---

## Workflow Engine

The workflow engine is implemented **entirely in the JS sandbox** — there is no new Rust execution path.

```
Workflow step → step.fn(ctx, prev) → ctx.tools.run() → ToolExecutor → Composio
```

The `WorkflowStepRecord` Rust type in `workflow/mod.rs` exists only for future persistence and replay features.

### Step execution model

- **Sequential** (`workflow.run`): steps run one after another; each step receives outputs of all previous steps.
- **Parallel** (`workflow.parallel`): all steps start simultaneously via `Promise.allSettled`; failures are isolated.

### Future work

- Persist step state to the database for long-running workflows.
- Replay from any step checkpoint.
- Timeout per step.

---

## Agent Engine

The agent engine is a **ReAct-style loop** (Reason + Act) that lets an LLM autonomously decide which tool to call next until the goal is achieved.

```
ctx.agent.run(options)
  │
  ├── Build messages array (system prompt + goal)
  │
  └── Loop (max maxSteps times):
        │
        ├── op_agent_llm_call(messages, toolDefs)
        │     └── agent::llm::call_llm()  →  OpenAI chat completions
        │           returns { done, tool?, input?, answer? }
        │
        ├── if done=true: return { answer, steps, output }
        │
        ├── ctx.tools.run(decision.tool, decision.input)
        │
        └── Append [assistant: tool call, user: tool result] to messages
```

### LLM protocol

The system prompt instructs the LLM to respond with JSON only:

```json
// To call a tool:
{ "done": false, "tool": "slack.send_message", "input": { "channel": "#ops", "text": "..." } }

// When complete:
{ "done": true, "answer": "Sent a welcome message to #ops" }
```

Non-JSON responses are treated as `done=true` with the text as the answer.

### LLM configuration

| Secret | Default | Description |
|--------|---------|-------------|
| `FLUXBASE_LLM_KEY` | — | Required. LLM provider API key |
| `FLUXBASE_LLM_URL` | `https://api.openai.com/v1/chat/completions` | Override endpoint |
| `FLUXBASE_LLM_MODEL` | `gpt-4o-mini` | Override model |

The `temperature` is fixed at `0.1` and `max_tokens` at `512` to keep agent decisions deterministic and cheap.

---

## Triggers

The trigger system maps incoming events to function IDs. It is currently modeled in Rust but not yet connected to a live event source — the gateway handles HTTP trigger routing.

### Trigger kinds

| Kind | Config | Description |
|------|--------|-------------|
| `http` | — | Any HTTP call to the function's gateway route |
| `webhook` | `source: "stripe"` | Authenticated webhook from an external service |
| `cron` | `schedule: "0 9 * * 1-5"` | Time-based schedule (cron expression, UTC) |

### TriggerRegistry

In-memory hash maps:
- `trigger_id → TriggerConfig`
- `webhook_source → Vec<trigger_id>` (fast lookup on incoming webhook)
- `function_id → Vec<trigger_id>` (reverse lookup for dashboard display)

### TriggerRouter

Receives `IncomingEvent { kind, source, payload, headers }` and returns `Vec<ResolvedTrigger { trigger_id, function_id, payload, source, tenant_id, project_id }`.

Fan-out is supported: multiple functions can listen to the same webhook source.

---

## Deterministic Execution Model

Fluxbase is designed to support **deterministic replay** — the ability to re-run a function invocation with exactly the same inputs and produce the same outputs. This is the foundation of `flux trace replay` and time-travel debugging.

### Sources of nondeterminism

All sources of nondeterminism are categorised as either **captured** (recorded in spans now) or **planned** (requires future `FluxContext` primitives).

| Source | Status | Notes |
|--------|--------|-------|
| External tool calls | Captured | Each `ctx.tools.run()` records the full response in a `tool` span |
| LLM responses | Captured | Each `op_agent_llm_call` response is logged as an `agent_step` span |
| Invocation payload | Captured | The raw `payload` is part of the execution request |
| Current time (`Date.now()`) | **Not captured** | Use `ctx.time.now()` when available |
| Random values (`Math.random()`) | **Not captured** | Use `ctx.random()` when available |
| UUID generation | **Not captured** | Use `ctx.id()` when available |
| Database reads | **Not captured** | Use `ctx.db.query()` when available |

### Determinism contract for user functions

Functions that use native JS APIs for time or randomness **cannot be deterministically replayed**. The platform will warn on deploy if known nondeterministic patterns are detected.

```js
// ❌ Not replayable
const ts = Date.now();
const id = Math.random().toString(36);

// ✅ Replayable (when ctx primitives are available)
const ts = ctx.time.now();
const id = ctx.id();
```

### Recorded execution envelope

For replay to work, the following must be recorded at invocation time (by the Gateway, not the Runtime):

- `function_id` + `deployment_id` (exact bundle version)
- `payload` (verbatim input)
- `secrets_snapshot` (values at time of execution, not re-fetched)
- `tool_responses` (each tool call result, in order)
- `llm_responses` (each agent step decision, in order)
- wall-clock timestamp (for `ctx.time.now()` seed)

The Runtime currently provides the execution boundary. The recording of the full envelope is a Gateway/API responsibility.

---

## Replay Execution Mode

Replay mode runs an existing function bundle with **pre-recorded inputs**, disabling all side effects so the execution is safe to repeat.

> **Status**: Architecture defined, not yet implemented in code.

### How replay differs from normal execution

| Aspect | Normal execution | Replay execution |
|--------|-----------------|------------------|
| Tool calls | Execute against real APIs | Return recorded response from trace |
| LLM calls | Make real API request | Return recorded decision from trace |
| Time (`ctx.time.now()`) | Returns `Date.now()` | Returns recorded timestamp |
| Random (`ctx.random()`) | Returns `Math.random()` | Returns recorded seed value |
| Database writes | Applied | Disabled or sandboxed |
| Logs | Post to control plane | Collected locally, returned inline |

### Replay request protocol

The Gateway signals replay mode via a request header:

```
x-flux-replay: true
x-flux-trace-id: <original trace ID to replay>
```

When `x-flux-replay: true` is present the Runtime must:

1. Read recorded tool/LLM responses from the trace store (via control plane)
2. Inject them as mock responses into the Deno op handlers instead of calling real APIs
3. Disable write-side effects in `ctx.db` operations
4. Return execution result + full log replay inline in the response body

### Replay Deno ops (planned)

```
op_execute_tool (replay)  →  read next tool response from trace recording
op_agent_llm_call (replay) →  read next LLM decision from trace recording
```

The JS sandbox code (`FluxContext`) does not change between normal and replay — the swap happens entirely at the Rust op level, making the implementation clean and auditable.

---

## Execution Comparison

`flux trace diff` compares two executions of the same request by fetching both traces from the control plane and diffing the state mutations that each produced.

### How it works

```
Trace A (original)                 Trace B (replay)
   GET /traces/:a                     GET /traces/:b
   GET /db/mutations?request_id=a     GET /db/mutations?request_id=b
         │                                   │
         └──────────────┬────────────────────┘
                        ▼
              compare runtime spans
              ─ status code
              ─ total_duration_ms
              ─ error_count

              compare mutation sets
              ─ same rows mutated?
              ─ same operations (insert/update/delete)?
              ─ per-field before/after JSONB diff
                        │
                        ▼
                    Verdict
                    FIXED | REGRESSED | BEHAVIOR CHANGED | IDENTICAL
```

### Verdict logic

| Condition | Verdict |
|-----------|--------|
| A had errors, B has none, mutations identical | `FIXED` |
| A had no errors, B has errors | `REGRESSED` |
| Neither has errors, but mutation sets differ | `BEHAVIOR CHANGED` |
| Status, duration within 5%, and all mutations match | `IDENTICAL` |

### What makes this more powerful than `git diff`

`git diff` shows you what _code_ changed between two commits. `flux trace diff` shows you what _data_ changed between two executions of the same endpoint — field by field, row by row, across every table the function touched. This is observable at the production-data layer, not just the code layer.

### Implementation

The diff is computed entirely in the CLI (`cli/src/trace_diff.rs`). The CLI calls:

1. `GET /traces/:id` twice (once per request ID) — extracts status, duration, error spans
2. `GET /db/mutations?request_id=` twice — gets the full mutation log for each execution
3. Pairs mutations by `(table_name, record_pk, version)` and calls `diff_json()` on each `before_state`/`after_state` pair
4. Classifies the verdict and renders the output

No server-side diff logic is required — the raw `before_state` and `after_state` JSONB columns in `state_mutations` contain everything needed.

---

## Observability & Logging

### Span types

Every log line emitted during execution carries a `span_type` and `source`:

| `span_type` | `source` | Emitted by | When |
|-------------|----------|-----------|------|
| `event` | `function` | `ctx.log()` | User code |
| `tool` | `tool` | `ctx.tools.run()` | Tool call (success or failure) |
| `workflow_step` | `workflow` | `ctx.workflow.*` | Each step completion |
| `agent_step` | `agent` | `ctx.agent.run()` | Each LLM reasoning step |
| `start` | `runtime` | execute_handler | `execution_start` — before V8 runs |
| `end` | `runtime` | execute_handler | `execution_end` — after all logs ship |

### Log shipping

All logs are fire-and-forget: `tokio::spawn` posts each span to:

```
POST {CONTROL_PLANE_URL}/internal/logs
X-Service-Token: {SERVICE_TOKEN}
{
  "source":           "function",
  "resource_id":      "function_uuid",
  "tenant_id":        "...",
  "project_id":       "...",
  "level":            "info",
  "message":          "...",
  "request_id":       "x-request-id value",
  "span_type":        "tool",

  // ── Trace fields (added by this implementation) ──────────────────────
  "span_id":          "unique UUID v4 for this span",
  "parent_span_id":   "span id from Gateway x-parent-span-id header",
  "code_sha":         "16-char bundle fingerprint for replay correlation",
  "execution_state":  "started | completed | error",
  "duration_ms":      45,
  "tool_name":        "slack.send_message",   // tool spans only
}
```

### Request ID and parent span tracing

`x-request-id` is propagated from the inbound request through every log span, enabling full end-to-end trace correlation across API → Runtime.

`x-parent-span-id` (forwarded by the Gateway) links every Runtime span back to the Gateway-level span, building a complete tree: `gateway_span → execution_start → [tool spans, log spans] → execution_end`.

---

## Runtime Instrumentation

This section documents the concrete span schema emitted by the runtime for each execution lifecycle event. These are the anchors for `flux trace`, `flux why`, and future `flux replay`.

### Lifecycle span sequence

For every function invocation, the following spans are guaranteed to be emitted in order:

```
execution_start  (span_type="start",  execution_state="started")
  ├── tool:slack.send_message 45ms  (span_type="tool",         execution_state="completed")
  ├── workflow:step1 120ms          (span_type="workflow_step", source="workflow")
  ├── ctx.log("done")               (span_type="event",         source="function")
  └── (on error) execution_error    (span_type="end",           execution_state="error")
execution_end    (span_type="end",    execution_state="completed", duration_ms=165)
```

### Span field reference

| Field | Type | All spans | Lifecycle only | Description |
|-------|------|-----------|---------------|-------------|
| `resource_id` | string | ✓ | | function UUID |
| `tenant_id` | UUID | ✓ | | Tenant context |
| `project_id` | UUID? | ✓ | | Project context |
| `request_id` | string? | ✓ | | Trace correlation ID — same across all spans for one invocation |
| `span_id` | string | ✓ | | Unique UUID v4 per span — required to build parent → child tree |
| `source` | string | ✓ | | `"runtime"` \| `"function"` \| `"tool"` \| `"workflow"` \| `"agent"` |
| `span_type` | string | ✓ | | `"start"` \| `"end"` \| `"event"` \| `"tool"` \| `"workflow_step"` \| `"agent_step"` |
| `level` | string | ✓ | | `"debug"` \| `"info"` \| `"warn"` \| `"error"` |
| `message` | string | ✓ | | Human-readable description |
| `parent_span_id` | string? | ✓ | | Gateway span ID — links this span to the parent for trace tree construction |
| `code_sha` | string? | ✓ | | 16-char bundle fingerprint — identifies the exact bundle version for replay |
| `execution_state` | string? | | ✓ | `"started"` \| `"completed"` \| `"error"` |
| `duration_ms` | u64? | | ✓ | Total execution duration (end/error spans) or tool call duration (tool spans) |
| `tool_name` | string? | | ✓ | Fluxbase tool name for `span_type=="tool"` spans |

### `bundle_sha` implementation

`code_sha` is computed from the bundle string using `DefaultHasher` (Rust std) with a fixed-width 16-char hex format. It changes whenever the bundle bytes change, providing a stable replay key without cryptographic overhead. Two identical deployments produce the same fingerprint.

### Error path tracing

When execution fails, the error span is emitted **before** returning the HTTP error response, so `flux why <request-id>` always has a terminal span to display:

```json
{
  "span_type":       "end",
  "execution_state": "error",
  "message":         "execution_error: FunctionExecutionError: ...",
  "span_id":         "<uuid-v4>",
  "duration_ms":     1243,
  "code_sha":        "a3f2c1b0d4e5f6a7",
  "parent_span_id":  "gw-span-abc123"
}
```

### Integration with `flux why`

Given a `request_id`, a query to the log store for all spans matching `request_id=X` ordered by timestamp gives:

1. Gateway span tree (from gateway) — links via `parent_span_id` = gateway's own `span_id`
2. `execution_start` span (runtime, first)
3. All tool/workflow/agent/log spans (runtime, middle) — each with its own `span_id`, all sharing the same `parent_span_id`
4. `execution_end` or `execution_error` span (runtime, last)

The three-identifier model (`request_id` / `span_id` / `parent_span_id`) is now complete and matches the OpenTelemetry / Jaeger span linking convention. `flux why` renders this as a flamegraph-style display.

---

## Performance Characteristics

These are empirical benchmarks from production (Cloud Run, `asia-south1`, 1 vCPU container). Actual numbers vary with container warmth, payload size, and external API latency.

### Per-request latency breakdown

| Component | Typical latency | Notes |
|-----------|----------------|-------|
| Bundle cache hit (function-level) | < 1 ms | 0 network calls |
| Bundle cache hit (deployment-level) | < 1 ms | 0 network calls |
| Secrets cache hit | < 1 ms | In-process LRU |
| `JsRuntime` startup (cold, container init) | 3–5 ms | Paid once per worker thread at startup |
| Isolate execution startup (warm) | < 0.5 ms | `OpState` swap + global sweep; V8 heap already warm |
| Secrets fetch (cache miss) | 5–15 ms | Control plane round-trip |
| Bundle fetch from R2/S3 (cache miss) | 30–100 ms | Object storage round-trip |
| Tool call via Composio | 100–2000 ms | External API dependent |
| LLM call (agent step) | 500–3000 ms | Model and provider dependent |

### Typical total invocation time

| Scenario | Expected duration |
|----------|------------------|
| Warm cache, no tools | 2–8 ms |
| Warm cache, 1 tool call | 150–500 ms |
| Cold bundle + cold secrets | 50–120 ms overhead |
| 3-step workflow, 3 tool calls | 400–2000 ms |
| Agent, 3 LLM steps, 3 tools | 2–8 s |

### Memory per worker

| Component | Approx. RSS |
|-----------|------------|
| V8 isolate (warm, per worker thread) | ~5 MB |
| OS thread stack | 8 MB |
| Per worker total | ~13–20 MB |
| 4-worker pool | ~60–80 MB |
| 8-worker pool | ~120–160 MB |

### Throughput

With `ISOLATE_WORKERS=4` and a channel buffer of 16:

- Sustained throughput: ~4 concurrent functions executing at once
- Burst capacity: 16 queued + 4 executing = 20 in-flight before backpressure applies
- At 30s max execution time: theoretical max of ~480 executions/minute under ideal conditions (no external calls)

---

## Configuration

All configuration is loaded from environment variables at startup (`Settings::load()`).

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `8081` | HTTP listen port |
| `CONTROL_PLANE_URL` | `http://localhost:8080` | Base URL of the API/Control Plane service |
| `SERVICE_TOKEN` | `stub_token` | Shared secret for inter-service authentication |
| `ISOLATE_WORKERS` | `min(2×CPU, 16)` | Number of V8 isolate worker threads |
| `COMPOSIO_API_KEY` | — | Platform-level Composio key (not user-supplied) |
| `COMPOSIO_ENTITY_ID` | `<tenant_id>` | Override Composio entity (for demo accounts) |
| `RUST_LOG` | `info,runtime=debug` | Log level filter |

---

## Dependencies

| Crate | Purpose |
|-------|---------|
| `axum 0.8` | HTTP server framework |
| `deno_core 0.354` | V8 isolate runtime (JavaScript execution) |
| `tokio` | Async runtime |
| `reqwest` | HTTP client (control plane + Composio + LLM calls) |
| `serde / serde_json` | Serialization |
| `lru` | LRU cache for bundles and secrets |
| `uuid` | Tenant/project/function ID types |
| `tracing / tracing-subscriber` | Structured logging |
| `tower-http` | Middleware (tracing, CORS) |
| `dotenvy` | `.env` file loading for local dev |

---

## Known Limitations & Improvement Areas

### Execution

- [ ] **No per-function resource limits** — CPU time and memory are not capped independently per tenant. A single runaway function can starve other workers.
- [ ] **Single Deno extension instance** — The `fluxbase` extension is rebuilt (`Cow::Owned(vec![...])`) on every `execute_function()` call. Consider pre-building it once.
- [ ] **No function output size limit** — Large return values serialize into memory without a cap.
- [ ] **Workflow step persistence** — Workflow steps run entirely in memory. A crash mid-workflow loses all progress. `WorkflowStepRecord` type exists but is not persisted.
- [ ] **No step-level timeout** — `ctx.workflow.run()` steps inherit the function's 30s global timeout; there is no per-step deadline.

### Tools

- [ ] **Static tool registry** — New integrations require a code change + redeploy. Consider loading tools dynamically from Composio's `/actions` discovery API.
- [ ] **Single Composio entity per tenant** — Multiple projects within a tenant share the same Composio entity ID, so connected accounts are not project-scoped.
- [ ] **No tool input validation** — Tool inputs are passed directly to Composio without schema validation, which can produce confusing Composio error messages.

### Agent

- [ ] **No tool parameter schemas in prompts** — `toolDefs` passed to the LLM have empty `parameters: {}`. Real parameter schemas would improve LLM accuracy.
- [ ] **No streaming** — Agent steps are blocking LLM calls. Streaming responses are not supported.
- [ ] **Memory grows per step** — The full message history is kept in-memory and sent on every LLM call. Long agents will hit `max_tokens` input limits.

### Caching

- [ ] **In-process cache** — `BundleCache` and `SecretsCache` are per-process. Multiple runtime replicas each maintain independent caches. A cache invalidation call hits only one replica.
- [ ] **No cache metrics** — Hit/miss rates are logged at `debug` level only. No Prometheus/metrics endpoint.

### Triggers

- [ ] **Trigger registry is in-memory, not persistent** — Triggers are lost on restart. The registry needs to be loaded from the database on startup.
- [ ] **Cron not connected** — `TriggerKind::Cron` is modeled but no scheduler feeds events into the router.
- [ ] **Webhook signature verification is a stub** — `enrich_webhook_payload` in `router.rs` adds a `_verified: false` field. Real HMAC verification is not implemented.

### Observability

- [x] **~~Error paths emitted no terminal span~~** — Fixed: `post_trace_span` is now called on all error code paths with `execution_state="error"` and `duration_ms` before the HTTP error response is returned. `flux why <request-id>` always has a terminal span.
- [ ] **Logs are fire-and-forget with no retry** — If the control plane is down or slow, execution logs are silently dropped.
- [ ] **No sampling** — All spans are posted regardless of volume. Consider sampling successful fast executions.
- [ ] **No duration histogram** — `duration_ms` is returned in the response and logged but not tracked as a metric.
