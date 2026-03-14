# Async I/O Multiplexing

Flux uses a cooperative I/O model where external calls (database, HTTP, queue, timers, filesystem) are handled by Rust while user code yields the CPU. Each worker handles many concurrent I/O-bound requests instead of blocking on one at a time.

Two runtime paths:
- **V8/Deno** for JavaScript/TypeScript — async ops via `deno_core`
- **WASM/Wasmtime** for all other languages — async fibers via WASI host imports

Both paths share the same Flux I/O layer: all effects go through Rust, all effects are observed, all effects are recorded.

## The Problem

Without multiplexing, each worker handles exactly one request at a time:

```
Worker 1:
  req1 starts → await ctx.db.query() → CPU IDLE for 50ms → responds
  req2 starts → (must wait until req1 finishes)

8 workers = max 8 concurrent requests, regardless of I/O wait
```

A single Node.js or Deno process can juggle thousands of concurrent I/O-bound requests on one thread. A naive isolate-per-request model cannot.

## The Insight

All external I/O in Flux routes through Rust:

| User code | Rust handler |
|-----------|-------------|
| `ctx.db.query()` | SQLx async query |
| `ctx.http.fetch()` | reqwest async HTTP |
| `ctx.queue.push()` | Postgres INSERT |
| `ctx.secrets.get()` | LRU cache + AES decrypt |
| `ctx.function.invoke()` | Internal recursive dispatch |
| `setTimeout()` / `sleep()` | `tokio::time::sleep` |

The user function never touches a socket or file descriptor directly. Rust owns all I/O. This means Rust can suspend the user function during I/O and resume it when the response arrives — while using the freed CPU for other requests.

## Architecture

```
┌──────────── Worker Thread ─────────────────────┐
│                                                 │
│  Event Loop (V8) / Async Fiber (WASM):          │
│                                                 │
│    req1: await ctx.db.query()    → [SUSPENDED]  │
│    req2: processing CPU work     → [RUNNING]    │
│    req3: await ctx.http.fetch()  → [SUSPENDED]  │
│    req4: await ctx.db.insert()   → [SUSPENDED]  │
│                                                 │
│  Rust async runtime (tokio):                    │
│    Postgres Future for req1      → [POLLING]    │
│    HTTP Future for req3          → [POLLING]    │
│    Postgres Future for req4      → [POLLING]    │
│                                                 │
│  Only req2 uses CPU.                            │
│  req1, req3, req4 are parked Rust Futures.      │
│  Worker handles all 4 concurrently.             │
└─────────────────────────────────────────────────┘
```

When a suspended request's I/O completes, the event loop picks it up on the next tick and resumes execution.

---

## V8 (JavaScript/TypeScript) — Async Ops

Deno's `deno_core` has native support for async ops. Flux-owned effects are registered as Rust async ops. When JavaScript `await`s them, V8 yields to the event loop.

```
Worker thread (1 JsRuntime, N concurrent requests for same function/version):

  tick 1: req1 starts → hits await ctx.db.query()
          → Rust async op starts Postgres Future
          → V8 yields (nothing else to run? pick up next task)
  tick 2: req2 injected → runs to completion → responds
  tick 3: req3 injected → hits await ctx.http.fetch()
          → Rust async op starts HTTP Future → V8 yields
  tick 4: req1's Postgres Future resolves → V8 resumes req1
  tick 5: req3's HTTP Future resolves → V8 resumes req3
```

### V8 Isolation Model

V8 workers use a **shared-heap-per-function** model, like a normal server runtime:

| Property | Behavior |
|----------|----------|
| Same function, same version | Requests share the V8 heap on one worker. Module-level state (caches, counters, connection pools) persists across requests — same as Node.js or Deno would behave. |
| Different function or version | Separate runtime. Worker recreates its `JsRuntime` when the bundle key changes. No heap sharing across function boundaries. |
| Cross-request `ctx` isolation | Each request gets its own `ctx` object with its own `request_id`, secrets, logs, and completion channel. `ctx` is scoped per-request; the heap is shared per-function. |

This is intentional. Shared module state is how every server runtime works. If a user writes `let cache = new Map()` at module level, it persists across requests on the same worker — exactly like `express` or `fastify` would behave. Flux does not fight this; it is the expected JS execution model.

### V8 Flux Op Surface

Each `ctx.*` method maps to a `deno_core` async op:

| JS code | Rust async op | What it does |
|---------|--------------|-------------|
| `ctx.db.query(sql, params)` | `op_db_query` | SQLx async query via data-engine |
| `ctx.db.<table>.find/insert/update/delete` | `op_db_query` | Typed helpers compile to `op_db_query` |
| `ctx.http.fetch(url, opts)` | `op_http_fetch` | reqwest + SSRF check + trace span |
| `ctx.queue.push(fn, payload, opts)` | `op_queue_push` | Resolve function → POST to queue service |
| `ctx.function.invoke(name, input)` | `op_function_invoke` | Recursive runtime dispatch with parent lineage |
| `ctx.secrets.get(key)` | Synchronous | LRU cache + AES decrypt (no async needed) |
| `ctx.log.info/warn/error(msg)` | Synchronous | Append to per-request log buffer |
| `setTimeout(fn, ms)` | `op_sleep` | `tokio::time::sleep().await` — V8 yields |

```rust
#[op2(async)]
async fn op_db_query(
    state: Rc<RefCell<OpState>>,
    #[string] request_id: String,
    #[serde] query: QueryRequest,
) -> Result<serde_json::Value, AnyError> {
    let (pool, span_tx) = {
        let s = state.borrow();
        let registry = s.borrow::<RequestRegistry>();
        let ctx = registry.get(&request_id)?;
        (ctx.db_pool.clone(), ctx.span_tx.clone())
    };
    let start = Instant::now();
    let rows = sqlx::query(&query.sql)
        .fetch_all(&pool)
        .await?;
    span_tx.send(Span::db(query.sql, start.elapsed()));
    Ok(rows_to_json(rows))
}

#[op2(async)]
async fn op_http_fetch(
    state: Rc<RefCell<OpState>>,
    #[string] request_id: String,
    #[serde] options: FetchOptions,
) -> Result<serde_json::Value, AnyError> {
    let (client, span_tx) = {
        let s = state.borrow();
        let registry = s.borrow::<RequestRegistry>();
        let ctx = registry.get(&request_id)?;
        (ctx.http_client.clone(), ctx.span_tx.clone())
    };
    validate_ssrf(&options.url)?;
    let start = Instant::now();
    let resp = client.request(options.method, &options.url)
        .headers(options.headers)
        .body(options.body)
        .send()
        .await?;
    span_tx.send(Span::http(options.url, resp.status(), start.elapsed()));
    Ok(response_to_json(resp).await?)
}
```

### V8 Multi-Request Event Loop

The worker loop injects tasks and continuously drives the event loop:

```rust
loop {
    tokio::select! {
        Some(task) = receiver.recv() => {
            if task.bundle_key != current_bundle_key {
                // Different function/version — recreate runtime
                drain_and_fail_inflight(&mut inflight);
                js_rt = create_js_runtime();
                current_bundle_key = task.bundle_key.clone();
            }
            if inflight.len() >= max_concurrent {
                task.reply.send(Err(backpressure_error()));
                continue;
            }
            inject_async_task(&mut js_rt, &mut inflight, task);
        }
        _ = js_rt.run_event_loop(PollEventLoopOptions::default()) => {
            // Completed tasks resolve via their oneshot channels
            inflight.retain(|t| !t.is_complete());
        }
    }
}
```

### V8 Task Injection Mechanism

`inject_async_task` is the critical piece. `deno_core`'s `JsRuntime` has no direct API
for "inject a new concurrent task into a running event loop." The mechanism is an
**op-fed JS bootstrap loop** that runs once per worker and polls for new tasks:

```js
// Injected once at worker startup (before any user code runs).
// Runs as a background async loop inside the same V8 event loop.
(async () => {
  while (true) {
    const task = await Deno.core.ops.op_next_task();
    if (task === null) break; // worker shutting down

    // Fire-and-forget: don't await here. This drops the task's promise
    // into the V8 microtask queue and immediately loops back to poll
    // for the next task. The event loop drives all in-flight promises
    // concurrently without this loop blocking on any of them.
    __flux_run_task(task).catch(err => {
      Deno.core.ops.op_task_error(task.request_id, String(err));
    });
  }
})();
```

`op_next_task` is a Rust async op backed by an `mpsc::Receiver`. When Rust calls
`inject_async_task`, it pushes to the sender side. The JS loop picks up the task on
the next event loop tick without blocking anything already in flight.

```rust
// Rust side — op_next_task suspends the JS loop until a task arrives.
// This is identical to how Deno's own worker thread bootstrapping works.
#[op2(async)]
async fn op_next_task(
    state: Rc<RefCell<OpState>>,
) -> Result<Option<serde_json::Value>, AnyError> {
    let mut rx = {
        let s = state.borrow();
        s.borrow::<TaskReceiver>().clone()
    };
    Ok(rx.recv().await)  // suspends V8 loop; tokio can drive other Futures
}

// inject_async_task just pushes to the channel.
// The JS bootstrap loop picks it up on the next tick.
fn inject_async_task(
    task_tx: &mpsc::Sender<serde_json::Value>,
    inflight: &mut InFlightMap,
    task: WorkerTask,
) {
    inflight.insert(task.request_id.clone(), task.reply);
    let _ = task_tx.send(task.into_json());
}
```

**Why this works without blocking:**

`op_next_task` suspends the bootstrap loop's `await` point, which parks that
particular async branch as a Rust `Future`. The tokio runtime is free to drive all
other in-flight Postgres/HTTP/sleep Futures while waiting. When a new task arrives,
tokio wakes the `op_next_task` Future, V8 resumes the bootstrap loop, fires the task
into the microtask queue, and immediately suspends again — all in one event loop tick.

**Failure isolation caveat:**

If user code executes a synchronous infinite loop (`while(true) {}`) it blocks the
V8 thread entirely. No other tasks on that worker can make progress until the timeout
fires and the worker is restarted. All in-flight requests on the worker are failed
with a 503 at that point. This is an inherent V8 limitation — JavaScript has no
preemptive scheduling. WASM does not have this problem because Wasmtime enforces fuel
limits per-invocation independently of other fibers.

### V8 Per-Request State

Each request is tracked in a `RequestRegistry` inside `OpState`, keyed by `request_id`:

```rust
struct RequestContext {
    request_id:   String,
    secrets:      HashMap<String, String>,
    logs:         Vec<LogLine>,
    span_tx:      mpsc::Sender<Span>,
    reply:        oneshot::Sender<Result<ExecutionResult, String>>,
    started_at:   Instant,
    timeout:      Duration,
    // Per-request seeded PRNG for deterministic replay
    prng_state:   u32,
}

struct RequestRegistry {
    requests: HashMap<String, RequestContext>,
}
```

Every async op receives the `request_id` explicitly from the JS `ctx` closure. The op uses the registry to resolve the correct per-request context. This ensures:
- Logs go to the right request
- Spans link to the right trace
- Secrets are scoped per request
- PRNG state is per-request (not shared across concurrent requests)

### V8 Worker Dispatch

Workers are not a generic FIFO. The dispatcher:

1. **Prefers** the least-loaded worker already running the same bundle key
2. **Falls back** to an idle worker (which recreates its runtime for the new bundle)
3. **Rejects** with 503 if no worker has capacity

This ensures maximum isolate reuse for high-repeat workloads while maintaining strict cross-function isolation.

---

## WASM — Async Fibers (Wasmtime)

Wasmtime supports async host calls via `async_support`. When a WASM module calls a host import, the WASM fiber is suspended and the tokio runtime can execute other work.

### WASM Isolation Model

WASM has **full per-request isolation**:

| Property | Behavior |
|----------|----------|
| Memory | Each request gets its own `Store` with its own linear memory. Zero shared state. |
| Host state | Each `Store` owns a `HostState` with request-specific secrets, logs, spans. |
| Compiled module | Shared (cached). Compilation happens once; instantiation is per-request. |
| Concurrency | Bounded by semaphore (`MAX_CONCURRENT_PER_WORKER`). Each request yields during host I/O via async fibers. |

This is strictly stronger isolation than V8. No request can observe another request's memory, even for the same function.

### WASM Flux Host Imports

| Import | Signature | Behavior |
|--------|-----------|----------|
| `fluxbase.db_query` | `(req_ptr, req_len, out_ptr, out_max) → i32` | Async — SQLx query via data-engine + span |
| `fluxbase.http_fetch` | `(req_ptr, req_len, out_ptr, out_max) → i32` | Async — reqwest + SSRF check + span |
| `fluxbase.queue_push` | `(req_ptr, req_len, out_ptr, out_max) → i32` | Async — resolve function → POST to queue |
| `fluxbase.function_invoke` | `(req_ptr, req_len, out_ptr, out_max) → i32` | Async — recursive dispatch with lineage |
| `fluxbase.sleep` | `(ms: i64)` | Async — `tokio::time::sleep().await` (fiber suspends) |
| `fluxbase.secrets_get` | `(key_ptr, key_len, out_ptr, out_max) → i32` | Sync — LRU cache + AES decrypt |
| `fluxbase.log` | `(level, msg_ptr, msg_len)` | Sync — append to `HostState::logs` |

All async host imports use `func_wrap_async` so the fiber suspends and frees the thread:

```rust
let mut config = Config::new();
config.async_support(true);
config.consume_fuel(true);

// Async host import — fiber suspends during I/O
linker.func_wrap_async("fluxbase", "http_fetch",
    |mut caller: Caller<HostState>, req_ptr: i32, req_len: i32, out_ptr: i32, out_max: i32| {
        Box::new(async move {
            let request = read_from_memory(&caller, req_ptr, req_len)?;
            validate_ssrf(&request.url)?;
            let start = Instant::now();
            let resp = caller.data().http_client.request(...)
                .send().await?;  // Fiber suspended — other requests run
            caller.data_mut().spans.push(Span::http(request.url, resp.status(), start.elapsed()));
            let body = response_to_json(resp).await?;
            write_to_memory(&mut caller, out_ptr, out_max, &body)?;
            Ok(body.len() as i32)
        })
    }
)?;

// Execution uses call_async — yields during host calls
let handle = instance.get_typed_func::<(i32, i32), i32>(&mut store, "handle")?;
let result = handle.call_async(&mut store, (ptr, len)).await?;
```

### WASM Concurrent Execution

With async support, WASM execution uses `tokio::spawn` instead of `spawn_blocking`:

```rust
// Async — yields during host I/O, thread freed for other requests
tokio::spawn(async move {
    execute_wasm_async(engine, module, params).await
})
```

The semaphore continues to bound total in-flight WASM executions. Each request gets its own `Store` and `HostState`. No shared memory, no coordination needed.

---

## Flux Op Surface (Both Runtimes)

Both V8 and WASM expose the same logical ops. The user-facing API is identical regardless of runtime:

| Effect | JS (V8) | WASM | Async | Traced | Replayable |
|--------|---------|------|-------|--------|------------|
| DB query | `ctx.db.query()` / `ctx.db.<table>.*` | `fluxbase.db_query` | Yes | Yes | Yes |
| HTTP fetch | `ctx.http.fetch()` | `fluxbase.http_fetch` | Yes | Yes | Yes |
| Queue push | `ctx.queue.push()` | `fluxbase.queue_push` | Yes | Yes | Yes |
| Function invoke | `ctx.function.invoke()` | `fluxbase.function_invoke` | Yes | Yes | Yes |
| Sleep / timer | `setTimeout()` | `fluxbase.sleep` / `poll_oneoff` | Yes | Yes | Yes |
| Secrets | `ctx.secrets.get()` | `fluxbase.secrets_get` | No | No | N/A |
| Logging | `ctx.log.*()` | `fluxbase.log` | No | Yes | N/A |

Every async op:
1. Resolves per-request context via `request_id`
2. Executes the I/O through Rust (tokio)
3. Emits a trace span with timing, target, and metadata
4. Records the I/O request/response for replay
5. Returns the result to user code

---

## Concurrency Characteristics

| Metric | V8 | WASM |
|--------|-----|------|
| Requests per worker (I/O-bound) | ~100+ (event loop) | ~100+ (async fibers) |
| 8 workers, 100 I/O requests | ~100 concurrent | ~100 concurrent |
| Worker idle during I/O | No — runs other requests | No — runs other fibers |
| CPU-bound limit | 8 concurrent (1 per worker) | 8 concurrent (1 per fiber active) |
| Request isolation (memory) | Shared heap per function/version | Full — own Store per request |
| Request isolation (ctx/state) | Per-request via `RequestRegistry` | Per-request via `HostState` |

I/O-bound concurrency scales with available memory (pending request contexts), not CPU cores.

## Request Lifecycle

```
1. Gateway receives HTTP request
2. Gateway routes to Runtime (in-process call in monolith)
3. Runtime dispatcher selects a worker:
   - V8: prefer worker already running same bundle key, or idle worker
   - WASM: any worker with semaphore capacity
4. Worker executes:
   - V8: inject request as async task into shared event loop
   - WASM: spawn async fiber with own Store
5. User function runs:
   a. CPU work → runs on worker thread
   b. ctx.db.query() → Rust async op/host import → user code SUSPENDED
   c. ctx.http.fetch() → Rust async op/host import → user code SUSPENDED
   d. While suspended, worker runs other requests
   e. I/O completes → Rust Future resolves → user code RESUMES
6. Function returns → execution record written → response sent
```

---

## Safety & Security

### Isolation

| Concern | V8 | WASM |
|---------|-----|------|
| Cross-request memory | Shared heap per function/version (like Node.js). Module-level state persists. | Full isolation — own `Store` per request. Zero shared memory. |
| Cross-**function** memory | Strict. Worker recreates `JsRuntime` on bundle key change. | Strict. Own `Store` per request by default. |
| `ctx` data (secrets, logs, spans) | Per-request. Keyed by `request_id` in `RequestRegistry`. Cleaned up on completion. | Per-request. Owned by `HostState` in `Store`. Dropped with `Store`. |
| PRNG state | Per-request seeded PRNG in `RequestContext`. Not shared across concurrent requests. | Per-request. Seeded in `HostState` or deterministic via `random_get` override. |
| Prototype pollution | Built-in prototypes frozen at worker startup. User code cannot poison `Object.prototype` etc. | N/A — WASM has no prototype chain. |

### CPU Limits

| Concern | V8 | WASM |
|---------|-----|------|
| Infinite loop / CPU hog | Per-request wall-clock timeout. V8 interrupt terminates the offending execution. | Fuel-based limit. Wasmtime traps with `OutOfFuel` when budget exhausted. |
| Timeout/fuel exhaustion effect | **Worker-wide reset.** The `JsRuntime` is recreated. All in-flight requests on that worker fail. This is the cost of shared-heap. | **Request-only.** The `Store` is dropped. Other in-flight fibers are unaffected. |

V8 worker-wide reset is the correct trade-off: a `while(true)` in JS monopolizes the event loop — there is no way to preempt synchronous JS execution without killing the runtime. This is inherent to V8, not a Flux limitation. The mitigation is:
- Keep `MAX_CONCURRENT_PER_WORKER` bounded (default 64) so a reset affects at most 64 requests
- Scale workers horizontally so a single reset does not take down the container
- Recreate the runtime immediately — the next request gets a clean isolate

### Concurrency Limits

| Concern | How it is handled |
|---------|-------------------|
| Runaway concurrency | Per-worker cap: `MAX_CONCURRENT_PER_WORKER` (default 64). Excess requests get 503. |
| Memory exhaustion | Pending request count is bounded by the concurrency cap. Backpressure propagates via 503 to the gateway. |
| Queue depth | Channel capacity is `workers × 4`. Callers block naturally when all workers are at capacity. |

### Network Security

| Concern | How it is handled |
|---------|-------------------|
| SSRF | Every outbound connection (V8 `op_http_fetch`, WASM `fluxbase.http_fetch`, raw socket connect) is validated against SSRF rules: deny private IPs (10.x, 172.16.x, 192.168.x, 169.254.x, localhost, [::1]), deny internal service URLs, configurable allow-list. |
| DNS rebinding | DNS resolution goes through Flux. Resolved IP is checked against SSRF deny-list before connecting. |
| TLS verification | Default `reqwest::Client` enforces TLS certificate verification. No `danger_accept_invalid_certs`. |

### Sandbox

| Category | Denied | Why |
|----------|--------|-----|
| **Listen/Accept** | `sock_listen`, `sock_accept`, `Deno.listen()` | Functions are request handlers, not servers |
| **Subprocess** | `proc_raise`, `Deno.Command()`, `exec`, `system`, `fork` | Sandbox escape risk |
| **Symlinks** | `path_symlink` | Path traversal risk |
| **Host filesystem** | Paths outside `/tmp/{request_id}/` | Isolation — no access to host or other requests |
| **Signals** | `proc_raise`, `Deno.kill()` | Cannot kill processes |
| **FFI / native libs** | `Deno.dlopen()` | No native library loading |
| **Environment variables** | `Deno.env.get()` / `environ_get` | Returns only injected secrets, not host env |
| **Process exit** | `Deno.exit()` / `proc_exit` | Terminates the **request**, not the worker |

### Filesystem Sandbox

Functions get a per-request virtual temp directory (`/tmp/{request_id}/`). All filesystem operations are path-validated:

- Paths are canonicalized and checked against the sandbox root
- No `..` traversal out of the sandbox
- No symlink creation (prevents escape)
- Files are cleaned up after execution completes
- No access to host filesystem, other requests' files, or system paths

### Secrets

- Encrypted at rest with AES-256-GCM
- Injected into the runtime via LRU cache (30s TTL)
- Never appear in execution records, logs, spans, or error messages
- Per-request scoped — each request sees only its project's secrets
- Synchronous access — no async needed, no suspension

---

## Observability

Because Flux handles all I/O, every operation automatically generates trace data without any user instrumentation.

### Trace Spans

Every Flux-owned effect emits a span:

| Effect | Span data captured |
|--------|--------------------|
| `ctx.db.query()` | SQL text, params hash, row count, duration, error if any |
| `ctx.http.fetch()` | Method, URL, status code, response size, duration |
| `ctx.queue.push()` | Function name, job ID, delay, duration |
| `ctx.function.invoke()` | Target function, nested request_id, duration |
| `setTimeout()` / `sleep()` | Requested duration, actual duration |
| Secrets access | Key name (not value), cache hit/miss |

This means `flux trace <id>` shows:

```
├─ function:create_user                     125ms
│  ├─ db:query     INSERT INTO users...      18ms  (1 row)
│  ├─ http:fetch   POST api.stripe.com       95ms  (200)
│  ├─ queue:push   send_welcome_email         2ms  (job_id=abc)
│  └─ sleep        10ms                      10ms
```

### Request Identity Threading

Every effect carries the full request lineage:

| Header / Field | Purpose |
|---------------|---------|
| `x-request-id` | UUID propagated from gateway through all Flux-owned effects |
| `parent_span_id` | Links child spans to parent for trace reconstruction |
| `project_id` | Tenant isolation — ensures spans are scoped to the correct project |
| `code_sha` | Deployed code version — links trace to exact source |

### Execution Records

Every function invocation produces an execution record:

```json
{
  "request_id": "uuid",
  "function_name": "create_user",
  "code_sha": "abc123",
  "input": { "email": "..." },
  "output": { "id": 1 },
  "error": null,
  "duration_ms": 125,
  "spans": [...],
  "created_at": "2026-03-14T..."
}
```

Spans, input/output, and error state are all captured atomically so `flux why <id>` can reconstruct the full execution.

---

## Replay

For `flux incident replay <id>`, Flux records all I/O at the op boundary:

```
Recording (during live execution):
  op_db_query("INSERT INTO users...", [...])  → { rows: 1 }        (18ms)
  op_http_fetch("POST api.stripe.com", ...)   → { status: 200, ... } (95ms)
  op_queue_push("send_welcome_email", ...)    → { job_id: "abc" }    (2ms)
  op_sleep(10)                                → ()                   (10ms)

Replay (during incident replay):
  op_db_query(...)    → return recorded { rows: 1 }       (no real DB call)
  op_http_fetch(...)  → return recorded { status: 200 }   (no real HTTP call)
  op_queue_push(...)  → return recorded { job_id: "abc" } (no real queue call)
  op_sleep(10)        → return immediately                 (no real sleep)
```

The function executes with the exact same I/O responses it saw in production. No mocking needed — the Flux op boundary IS the mock boundary.

### Replay guarantees

| What | Guaranteed |
|------|-----------|
| Flux-owned effects (db, http, queue, invoke, sleep) | Yes — recorded and replayed exactly |
| Deterministic randomness (`Math.random`, `crypto.randomUUID`) | Yes — seeded per-request via `execution_seed` |
| Module-level state in V8 (caches, counters) | Best-effort — replay starts with clean module state. If the function depends on accumulated state from prior requests, replay may diverge. |
| Wall-clock time | Best-effort — `Date.now()` returns real time during replay. `clock_time_get` is not mocked. Replay is timing-approximate, not timing-exact. |

### What replay does NOT cover

- Raw Deno ops (`Deno.connect`, `fetch` directly) if user bypasses `ctx.*` — these are not recorded
- Side effects in module-level V8 state from prior requests
- Non-deterministic iteration order of JS `Map`/`Set` (V8-dependent)
- Time-dependent branching based on `Date.now()`

These are inherent limitations of replaying a shared-heap runtime. WASM replay is stricter because each request starts with clean memory.

---

## Complete I/O Surface

Flux intercepts all I/O at two layers depending on runtime:

### WASM — WASI Syscall Surface

WASM modules cannot do I/O directly. Every I/O operation compiles down to WASI syscalls. Flux implements them as async host imports:

#### Network I/O

| WASI Syscall | What it does | Flux async handler |
|-------------|-------------|-------------------|
| `sock_open` | Create TCP/UDP socket | Allocate fd entry in HostState |
| `sock_connect` | Connect to remote host | `TcpStream::connect().await` + SSRF check + span |
| `sock_send` | Send bytes (TCP) | `stream.write_all().await` |
| `sock_recv` | Receive bytes (TCP) | `stream.read().await` |
| `sock_sendto` | Send datagram (UDP) | `UdpSocket::send_to().await` |
| `sock_recvfrom` | Receive datagram (UDP) | `UdpSocket::recv_from().await` |
| `sock_shutdown` | Half-close connection | `stream.shutdown().await` |
| `sock_close` | Destroy socket | Drop stream + close span |
| `sock_listen` | Bind + listen | **Denied** — functions are not servers |
| `sock_accept` | Accept connection | **Denied** |
| `sock_getaddrinfo` | DNS resolution | `tokio::net::lookup_host().await` + SSRF check |

Every language's standard networking (HTTP clients, Redis drivers, gRPC stubs, database drivers, MQTT, SMTP, Kafka) compiles to these syscalls. Zero driver-specific code needed in Flux.

#### Filesystem I/O

| WASI Syscall | What it does | Flux async handler |
|-------------|-------------|-------------------|
| `fd_read` | Read from file descriptor | `tokio::fs::File::read().await` (sandboxed) |
| `fd_write` | Write to file descriptor | `tokio::fs::File::write_all().await` (sandboxed) |
| `fd_seek` | Seek in file | Sync (in-memory position) |
| `fd_close` | Close file descriptor | Drop handle |
| `fd_sync` / `fd_datasync` | Flush to disk | `file.sync_all().await` |
| `fd_readdir` | List directory | `tokio::fs::read_dir().await` (sandboxed) |
| `fd_prestat_get` | Query pre-opened directories | Sync (HostState lookup) |
| `path_open` | Open file by path | `tokio::fs::File::open().await` (sandboxed, path validated) |
| `path_create_directory` | Create directory | `tokio::fs::create_dir().await` (sandboxed) |
| `path_remove_directory` | Remove directory | `tokio::fs::remove_dir().await` (sandboxed) |
| `path_rename` | Rename file/dir | `tokio::fs::rename().await` (sandboxed) |
| `path_unlink_file` | Delete file | `tokio::fs::remove_file().await` (sandboxed) |
| `path_readlink` | Read symlink | `tokio::fs::read_link().await` (sandboxed) |
| `path_symlink` | Create symlink | **Denied** — security risk |
| `path_filestat_get` | Stat file | `tokio::fs::metadata().await` |

#### Clocks, Timers, Environment

| WASI Syscall | Flux handler |
|-------------|-------------|
| `clock_time_get` (realtime) | Sync — `SystemTime::now()` |
| `clock_time_get` (monotonic) | Sync — `Instant::now()` |
| `clock_res_get` | Sync — nanosecond precision |
| `poll_oneoff` (clock subscription) | **Async** — `tokio::time::sleep().await` (fiber suspends) |
| `poll_oneoff` (I/O subscriptions) | **Async** — converts to tokio Futures, `select!`, resumes on completion |
| `random_get` | Sync — `getrandom::getrandom()` (seedable for replay) |
| `args_get` / `args_sizes_get` | Sync — function metadata |
| `environ_get` / `environ_sizes_get` | Sync — injected secrets/config only |
| `proc_exit` | Terminates the **request**, not the worker |
| `proc_raise` | **Denied** |
| `sched_yield` | Yields fiber — other requests run |

`poll_oneoff` is WASI's equivalent of `epoll`/`kqueue`. It is the core multiplexing syscall — every language's async runtime eventually calls it. Flux converts each subscription to a tokio Future, suspends the fiber, and resumes when any complete.

### V8 (JavaScript/TypeScript) — Deno Op Surface

For the JS/TS path, Flux registers custom Deno ops. Standard Deno networking and filesystem ops are also available, sandboxed:

#### Network

| JS API | Deno Op | Flux behavior |
|--------|---------|--------------|
| `fetch(url)` | `op_fetch` | Async — reqwest + SSRF check + span |
| `new WebSocket(url)` | `op_ws_*` | Async — tokio-tungstenite + span |
| `Deno.connect({port})` | `op_net_connect` | Async — TcpStream + SSRF check |
| `Deno.connectTls({port})` | `op_tls_connect` | Async — TLS + SSRF check |
| `conn.read(buf)` | `op_net_read` | Async — stream.read().await |
| `conn.write(data)` | `op_net_write` | Async — stream.write().await |
| `Deno.resolveDns(name)` | `op_dns_resolve` | Async — DNS lookup |
| `Deno.listen({port})` | `op_net_listen` | **Denied** — functions are not servers |

npm packages like `ioredis`, `pg`, `mysql2`, `kafkajs`, `amqplib` work natively — their network calls go through Deno's ops which are already async.

#### Filesystem

| JS API | Deno Op | Flux behavior |
|--------|---------|--------------|
| `Deno.readFile(path)` | `op_read_file` | Async — sandboxed to `/tmp/{request_id}/` |
| `Deno.writeFile(path, data)` | `op_write_file` | Async — sandboxed |
| `Deno.readTextFile(path)` | `op_read_text_file` | Async — sandboxed |
| `Deno.stat(path)` | `op_stat` | Async — sandboxed |
| `Deno.mkdir(path)` | `op_mkdir` | Async — sandboxed |
| `Deno.remove(path)` | `op_remove` | Async — sandboxed |
| `Deno.readDir(path)` | `op_read_dir` | Async — sandboxed |
| `Deno.rename(from, to)` | `op_rename` | Async — sandboxed |

#### Timers and Scheduling

| JS API | Deno Op | Flux behavior |
|--------|---------|--------------|
| `setTimeout(fn, ms)` | `op_sleep` | Async — `tokio::time::sleep().await`, V8 yields |
| `setInterval(fn, ms)` | `op_sleep` (repeated) | Async — yields between intervals |
| `queueMicrotask(fn)` | V8 microtask queue | Sync — runs before next event loop tick |
| `Promise.all([...])` | V8 + multiple async ops | Async — all pending ops polled concurrently |

#### Subprocess and System

| JS API | Deno Op | Flux behavior |
|--------|---------|--------------|
| `Deno.Command(cmd).spawn()` | `op_spawn` | **Denied** — no subprocess execution |
| `Deno.env.get(key)` | `op_env_get` | Returns injected secrets only |
| `Deno.exit(code)` | `op_exit` | Terminates the **request**, not the worker |
| `crypto.getRandomValues(buf)` | `op_crypto_random` | Sync — kernel entropy |
| `crypto.subtle.digest()` | `op_crypto_*` | Sync — CPU-bound crypto |

---

## Configuration

| Env Variable | Default | Purpose |
|-------------|---------|---------|
| `ISOLATE_WORKERS` | `2 × CPU cores` (clamped [2, 16]) | Number of V8 worker threads |
| `MAX_CONCURRENT_PER_WORKER` | `64` | Max simultaneous I/O-bound requests per worker |
| `REQUEST_TIMEOUT_SECONDS` | `30` | Per-request wall clock timeout |
| `WASM_FUEL_LIMIT` | `1_000_000_000` | CPU fuel units per WASM invocation |
| `WASM_HTTP_ALLOWED_HOSTS` | (empty = deny all) | Comma-separated hosts WASM may fetch. `*` = allow all. |

---

## Architecture Diagram

```
┌───────────────────────────────────────────────────────────────┐
│  User Function (JS/TS or any WASM language)                    │
│  ctx.db.query() / ctx.http.fetch() / ctx.queue.push()          │
├───────────────────────────────────────────────────────────────┤
│  Flux SDK / ctx object                                         │
│  Typed DB helpers, HTTP wrapper, queue wrapper, logging         │
├───────────────────────────────────────────────────────────────┤
│  Deno Async Ops (V8)        │ WASI Host Imports (WASM)         │
│  op_db_query                │ fluxbase.db_query                │
│  op_http_fetch              │ fluxbase.http_fetch              │
│  op_queue_push              │ fluxbase.queue_push              │
│  op_function_invoke         │ fluxbase.function_invoke         │
│  op_sleep                   │ fluxbase.sleep / poll_oneoff     │
├─────────────────────────────┴─────────────────────────────────┤
│  Flux I/O Layer (Rust, async)                                  │
│                                                                │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────────┐ │
│  │ Security │ │ Observe  │ │ Record   │ │ Sandbox           │ │
│  │ SSRF     │ │ Spans    │ │ I/O for  │ │ Path validation   │ │
│  │ ACLs     │ │ Timing   │ │ replay   │ │ Deny list         │ │
│  │ Limits   │ │ Bytes    │ │          │ │ Filesystem jail   │ │
│  └──────────┘ └──────────┘ └──────────┘ └───────────────────┘ │
│                                                                │
├────────────────────────────────────────────────────────────────┤
│  Tokio Async Runtime                                           │
│  SQLx · reqwest · TcpStream · UdpSocket · TLS · DNS · Timer    │
└────────────────────────────────────────────────────────────────┘
```

---

## Why This Matters

This design means Flux achieves Node.js-level I/O concurrency while maintaining:

- **Full observability** — every Flux-owned effect is a span, automatically, in every language
- **Rust-level I/O performance** — all I/O uses tokio + native async drivers
- **Multi-tenant safety** — one user's slow API call does not block another user's function
- **Language transparency** — users write normal code with normal libraries; Flux is the I/O layer
- **Deterministic replay** — Flux-owned effects are recorded and replayed for debugging
- **Per-request isolation** — WASM: full memory isolation; V8: shared heap per function (like Node.js) with per-request `ctx` isolation

The isolation model is honest:
- WASM gives you full per-request memory isolation
- V8 gives you shared module state (caches, pools) with per-request `ctx` scoping — exactly like a normal JS server
- Both give you per-request trace, per-request spans, and per-request replay of Flux-owned effects

The user writes normal code in their language. Flux handles the rest.
