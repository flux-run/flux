# Async I/O Multiplexing

Flux uses a cooperative I/O model where external calls (database, HTTP, queue) are handled by Rust while user code yields the CPU. This allows each worker to handle many concurrent I/O-bound requests instead of blocking on one at a time.

## The Problem

Without multiplexing, each worker handles exactly one request at a time:

```
Worker 1:
  req1 starts → await ctx.db.query() → CPU IDLE for 50ms → responds
  req2 starts → (must wait until req1 finishes)

8 workers = max 8 concurrent requests, regardless of I/O wait
```

A single Node.js or Deno process juggle thousands of concurrent I/O-bound requests on one thread. A naive isolate-per-request model cannot.

## The Insight

All external I/O in Flux already routes through Rust:

| User code | Rust handler |
|-----------|-------------|
| `ctx.db.query()` | SQLx async query |
| `ctx.http.fetch()` | reqwest async HTTP |
| `ctx.queue.push()` | Postgres INSERT |
| `ctx.secrets.get()` | LRU cache + AES decrypt |
| `ctx.function.invoke()` | Internal dispatch |

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

## V8 (JavaScript/TypeScript) — Async Ops

Deno's `deno_core` has native support for async ops. External calls are registered as Rust async ops. When JavaScript `await`s them, V8 yields to the event loop.

```
Worker thread (1 JsRuntime, N concurrent requests):

  tick 1: req1 starts → hits await ctx.db.query()
          → Rust async op starts Postgres Future
          → V8 yields (nothing else to run? pick up next task)
  tick 2: req2 injected → runs to completion → responds
  tick 3: req3 injected → hits await ctx.http.fetch()
          → Rust async op starts HTTP Future → V8 yields
  tick 4: req1's Postgres Future resolves → V8 resumes req1
  tick 5: req3's HTTP Future resolves → V8 resumes req3
```

### Implementation: Registering Async Ops

Each `ctx.*` method maps to a `deno_core` async op:

```rust
// ctx.db.query() → Rust async op
#[op2(async)]
async fn op_db_query(
    state: Rc<RefCell<OpState>>,
    #[string] sql: String,
    #[serde] params: Vec<serde_json::Value>,
) -> Result<serde_json::Value, AnyError> {
    let pool = {
        let s = state.borrow();
        s.borrow::<DbPool>().clone()
    };
    // This is a real async Postgres query.
    // While it awaits, V8 can run other requests.
    let rows = sqlx::query(&sql)
        .fetch_all(&pool)
        .await?;
    Ok(rows_to_json(rows))
}

// ctx.http.fetch() → Rust async op
#[op2(async)]
async fn op_http_fetch(
    state: Rc<RefCell<OpState>>,
    #[string] url: String,
    #[serde] options: FetchOptions,
) -> Result<serde_json::Value, AnyError> {
    let client = {
        let s = state.borrow();
        s.borrow::<reqwest::Client>().clone()
    };
    // Real async HTTP call. V8 yields here.
    let resp = client.request(options.method, &url)
        .headers(options.headers)
        .body(options.body)
        .send()
        .await?;
    Ok(response_to_json(resp).await?)
}
```

### Implementation: Multi-Request Event Loop

The worker loop changes from "one request at a time" to "inject tasks, run event loop":

```rust
// Before: serial execution
loop {
    let task = receiver.recv().await;
    let result = execute_with_runtime(&mut runtime, task).await;
    task.response_tx.send(result);
}

// After: concurrent execution
loop {
    tokio::select! {
        // Accept new tasks when available
        Some(task) = receiver.recv() => {
            inject_async_task(&mut runtime, task);
        }
        // Drive the event loop (resolves pending I/O, runs callbacks)
        _ = runtime.run_event_loop(PollEventLoopOptions::default()) => {
            // Completed tasks send results via their response channels
        }
    }
}
```

### Per-Request Isolation Within Shared Event Loop

Each request runs in its own async scope with its own context. Requests on the same event loop cannot access each other's state:

- Each request gets a unique `request_id` threaded through its ops
- `OpState` uses the `request_id` to route to the correct per-request context (secrets, logs, DB pool)
- The `globalThis` sweep between requests is no longer needed during concurrent execution — each request's scope is an isolated async closure
- On completion, the request's context is cleaned up and its result is sent via oneshot channel

## WASM — Async Fibers (Wasmtime)

Wasmtime supports async host calls via `async_support`. When a WASM module calls a host import (like `http_fetch`), the WASM fiber is suspended and the worker can execute other WASM instances.

### Implementation: Async Engine Config

```rust
let mut config = Config::new();
config.async_support(true);   // Enable fiber-based async
config.consume_fuel(true);    // Keep CPU limits
```

### Implementation: Async Host Imports

```rust
// Before: synchronous (blocks the thread)
linker.func_wrap("fluxbase", "http_fetch",
    |mut caller: Caller<HostState>, req_ptr: u32, req_len: u32, out_ptr: u32, out_max: u32| -> u32 {
        let response = blocking_http_call(url);  // Thread blocked!
        write_to_memory(&mut caller, out_ptr, &response);
        response.len() as u32
    }
)?;

// After: async (fiber suspends, worker freed)
linker.func_wrap_async("fluxbase", "http_fetch",
    |mut caller: Caller<HostState>, req_ptr: u32, req_len: u32, out_ptr: u32, out_max: u32| {
        Box::new(async move {
            let response = reqwest::get(url).await;  // Fiber suspended!
            write_to_memory(&mut caller, out_ptr, &response);
            Ok(response.len() as u32)
        })
    }
)?;

// Execution uses call_async instead of call
let handle = instance.get_typed_func::<(u32, u32), u32>(&mut store, "handle")?;
let result = handle.call_async(&mut store, (ptr, len)).await?;  // Yields during host calls
```

### Implementation: Concurrent WASM Execution

With async support, multiple WASM instances can share a tokio task without dedicated threads:

```rust
// Before: spawn_blocking (1 OS thread per request)
tokio::task::spawn_blocking(move || {
    execute_wasm_sync(engine, module, input)  // Blocks thread
})

// After: async execution (yields during host I/O)
tokio::spawn(async move {
    execute_wasm_async(engine, module, input).await  // Yields on I/O
})
```

The `spawn_blocking` pool is still available as a fallback for WASM modules that do pure CPU work, but I/O-heavy modules benefit from async execution.

## Concurrency Characteristics

### Before (Current)

| Metric | V8 | WASM |
|--------|-----|------|
| Requests per worker | 1 | 1 |
| 8 workers, 100 I/O requests | 8 concurrent, 92 queued | 8 concurrent, 92 queued |
| Worker idle during I/O | Yes | Yes |

### After (Async I/O Multiplexing)

| Metric | V8 | WASM |
|--------|-----|------|
| Requests per worker (I/O-bound) | ~100+ | ~100+ |
| 8 workers, 100 I/O requests | ~100 concurrent | ~100 concurrent |
| Worker idle during I/O | No — runs other requests | No — runs other fibers |
| CPU-bound limit | 8 concurrent (unchanged) | 8 concurrent (unchanged) |

I/O-bound concurrency scales with available memory (pending request contexts), not CPU cores.

## Request Lifecycle (After)

```
1. Gateway receives HTTP request
2. Gateway routes to Runtime (in-process call in monolith)
3. Runtime picks a worker with capacity
4. Worker injects request as async task into event loop (V8) or spawns async fiber (WASM)
5. User function executes:
   a. CPU work → runs on worker thread
   b. ctx.db.query() → Rust async op, user code SUSPENDED
   c. ctx.http.fetch() → Rust async op, user code SUSPENDED
   d. While suspended, worker picks up other requests
   e. I/O completes → Rust Future resolves → user code RESUMES
6. Function returns → execution record written → response sent
```

## Safety Guarantees

| Concern | How it is handled |
|---------|-------------------|
| Cross-request data leakage | Each request has isolated OpState / HostState keyed by request_id |
| CPU starvation (one request hogs CPU) | Fuel limits (WASM) and timeout (V8) still enforced per request |
| Runaway concurrency | Per-worker request cap (configurable, e.g., max 64 concurrent per worker) |
| Memory exhaustion | Pending request count bounded; backpressure via 503 when limit reached |
| Prototype pollution (V8) | Prototypes frozen at startup; each request runs in async closure, not shared global scope |
| `while(true)` in user code | Timeout kills the request, not the worker. Other concurrent requests on same worker are unaffected (V8 event loop preemption for async ticks) |

## Configuration

| Env Variable | Default | Purpose |
|-------------|---------|---------|
| `ISOLATE_WORKERS` | `2 × CPU cores` (clamped [2, 16]) | Number of worker threads |
| `MAX_CONCURRENT_PER_WORKER` | `64` | Max simultaneous I/O-bound requests per worker |
| `REQUEST_TIMEOUT_SECONDS` | `30` | Per-request wall clock timeout |
| `WASM_FUEL_LIMIT` | `1_000_000_000` | CPU fuel units per WASM invocation |

## Complete I/O Surface

Flux intercepts **all** I/O — not just network calls. Every syscall that touches the outside world, blocks, or waits must go through Rust so user code can be suspended and resumed transparently.

The principle: **if it would block a thread in a normal program, Flux makes it async.**

### WASM — WASI Syscall Surface

WASM modules cannot do I/O directly. Every I/O operation in every language compiles down to WASI syscalls. Flux implements all of them as async host imports:

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
| `sock_getaddrinfo` | DNS resolution | `tokio::net::lookup_host().await` |

Every language's standard networking (HTTP clients, Redis drivers, gRPC stubs, database drivers, MQTT, SMTP, Kafka) compiles to these syscalls. Zero driver-specific code needed in Flux.

#### Filesystem I/O

| WASI Syscall | What it does | Flux async handler |
|-------------|-------------|-------------------|
| `fd_read` | Read from file descriptor | `tokio::fs::File::read().await` (sandboxed) |
| `fd_write` | Write to file descriptor | `tokio::fs::File::write_all().await` (sandboxed) |
| `fd_seek` | Seek in file | Sync (in-memory position, no I/O) |
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

**Filesystem sandbox:** Functions only see a per-request virtual temp directory (`/tmp/{request_id}/`). No access to host filesystem, other requests' files, or system paths. Files are cleaned up after execution.

#### Clocks and Timers

| WASI Syscall | What it does | Flux handler |
|-------------|-------------|-------------|
| `clock_time_get` (realtime) | Current wall clock time | Sync — returns `SystemTime::now()` |
| `clock_time_get` (monotonic) | Monotonic timer | Sync — returns `Instant::now()` |
| `clock_res_get` | Clock resolution | Sync — returns nanosecond precision |
| `poll_oneoff` (clock subscription) | `sleep()` / `setTimeout()` | **Async** — `tokio::time::sleep().await` (fiber suspends) |

`sleep()` in any language compiles to `poll_oneoff` with a clock subscription. Flux implements it as `tokio::time::sleep().await` — the fiber suspends, other requests run, and it resumes when the timer fires.

```
User writes:            What happens:

time.Sleep(5*time.Second)  → WASI poll_oneoff(clock, 5s)
                             → tokio::time::sleep(Duration::from_secs(5)).await
                             → fiber SUSPENDED — CPU runs other requests for 5 seconds
                             → timer fires → fiber RESUMES
```

#### Randomness

| WASI Syscall | What it does | Flux handler |
|-------------|-------------|-------------|
| `random_get` | Fill buffer with random bytes | Sync — `getrandom::getrandom()` (no I/O, kernel entropy) |

Randomness is synchronous and safe — no suspension needed. Optionally, Flux can seed deterministically for `flux incident replay`.

#### Process and Environment

| WASI Syscall | What it does | Flux handler |
|-------------|-------------|-------------|
| `args_get` / `args_sizes_get` | Command-line args | Sync — returns function metadata |
| `environ_get` / `environ_sizes_get` | Environment variables | Sync — returns injected secrets/config |
| `proc_exit` | Terminate process | Terminates the **request**, not the worker |
| `proc_raise` | Send signal | **Denied** |
| `sched_yield` | Yield CPU | Yields fiber — other requests can run |

#### Poll (The Async Primitive)

| WASI Syscall | What it does | Flux handler |
|-------------|-------------|-------------|
| `poll_oneoff` | Wait for multiple I/O events | **The core multiplexing syscall** |

`poll_oneoff` is WASI's equivalent of `epoll`/`kqueue`. When a WASM module calls `poll_oneoff` with a list of subscriptions (sockets ready to read, timers to fire, files ready to write), Flux:

1. Converts each subscription to a tokio `Future`
2. Suspends the WASM fiber
3. Runs `tokio::select!` on all Futures
4. When any complete, resumes the fiber with the results

This single syscall is what makes `select()`, `poll()`, async I/O multiplexing, `Promise.all()`, and event loops work inside WASM. Every language's async runtime eventually calls this.

### V8 (JavaScript/TypeScript) — Deno Op Surface

For the JS/TS path, Deno already provides non-blocking I/O through its op system. Flux wraps or replaces these ops:

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

This means npm packages like `ioredis`, `pg`, `mysql2`, `kafkajs`, `amqplib`, `mqtt` all work natively — their network calls go through Deno's ops which are already async.

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
| `Promise.resolve()` | V8 microtask queue | Sync |
| `Promise.all([...])` | V8 + multiple async ops | Async — all pending ops polled concurrently |
| `requestAnimationFrame` | **Denied** — no browser context | |

#### Subprocess and System

| JS API | Deno Op | Flux behavior |
|--------|---------|--------------|
| `Deno.Command(cmd).spawn()` | `op_spawn` | **Denied** — no subprocess execution |
| `Deno.env.get(key)` | `op_env_get` | Returns injected secrets only |
| `Deno.exit(code)` | `op_exit` | Terminates the **request**, not the worker |
| `crypto.getRandomValues(buf)` | `op_crypto_random` | Sync — kernel entropy |
| `crypto.subtle.digest()` | `op_crypto_*` | Sync — CPU-bound crypto |

### What Gets Denied

Not all I/O is allowed. Functions are sandboxed:

| Category | Denied | Why |
|----------|--------|-----|
| **Listen/Accept** | `sock_listen`, `sock_accept`, `Deno.listen()` | Functions are request handlers, not servers |
| **Subprocess** | `proc_raise`, `Deno.Command()` | No shell access from user code |
| **Raw exec** | `exec`, `system`, fork | Sandbox escape risk |
| **Symlinks** | `path_symlink` | Path traversal risk |
| **Host filesystem** | Paths outside `/tmp/{request_id}/` | Isolation — no access to host or other requests |
| **Signals** | `proc_raise`, `Deno.kill()` | Cannot kill processes |
| **FFI** | `Deno.dlopen()` | No native library loading |

### Observability From I/O Interception

Because Flux handles all I/O, every operation automatically generates trace data:

| I/O Type | Span data captured |
|----------|--------------------|
| Network connect | Remote host, port, protocol, connect duration |
| Network send/recv | Bytes transferred, latency per read/write |
| DNS resolution | Hostname, resolved IPs, lookup duration |
| File read/write | Path, bytes, duration |
| Sleep/timer | Requested duration, actual duration |
| Socket close | Total connection duration, total bytes in/out |

This means `flux trace <id>` can show:

```
├─ function:create_user                     125ms
│  ├─ net:connect  postgres:5432              2ms
│  ├─ net:send     INSERT INTO users...       0.1ms
│  ├─ net:recv     1 row                     18ms
│  ├─ net:connect  redis:6379                 1ms
│  ├─ net:send     SET user:123 ...           0.1ms
│  ├─ net:recv     OK                         0.5ms
│  ├─ net:connect  api.stripe.com:443         5ms  (TLS)
│  ├─ net:send     POST /v1/customers         0.1ms
│  ├─ net:recv     200 OK                    95ms
│  └─ sleep        10ms                      10ms
```

Without any user instrumentation. Flux sees it all because it IS the I/O layer.

### Replay From I/O Recording

For `flux incident replay <id>`, Flux can record all I/O at the syscall level:

```
Recording (during live execution):
  sock_connect(postgres:5432) → fd=3
  sock_send(fd=3, "INSERT INTO users...") → 45 bytes
  sock_recv(fd=3) → "1 row affected" (18ms)
  sock_connect(redis:6379) → fd=4
  ...

Replay (during incident replay):
  sock_connect(postgres:5432) → return recorded fd=3
  sock_send(fd=3, ...) → recorded 45 bytes
  sock_recv(fd=3) → return recorded "1 row affected" (no real network call)
  ...
```

The function executes with the exact same I/O responses it saw in production. No mocking needed — the syscall layer IS the mock boundary.

### Architecture Diagram

```
┌───────────────────────────────────────────────────────────────┐
│  User Function (any language, any driver, any library)         │
│  redis.get() / http.get() / fs.readFile() / sleep(5)          │
├───────────────────────────────────────────────────────────────┤
│  Language Standard Library                                     │
│  (net, http, tls, fs, time, crypto, os)                        │
├───────────────────────────────────────────────────────────────┤
│  WASI Syscalls (WASM) │ Deno Ops (V8)                          │
│  sock_* fd_* poll_*   │ op_fetch op_net_* op_sleep op_fs_*     │
├───────────────────────┴───────────────────────────────────────┤
│  Flux I/O Layer (Rust, async)                                  │
│                                                                │
│  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌───────────────────┐ │
│  │ Security │ │ Observe  │ │ Record   │ │ Sandbox           │ │
│  │ SSRF     │ │ Spans    │ │ I/O for  │ │ Path validation   │ │
│  │ ACLs     │ │ Bytes    │ │ replay   │ │ Deny list         │ │
│  │ Limits   │ │ Timing   │ │ mock     │ │ Filesystem jail   │ │
│  └──────────┘ └──────────┘ └──────────┘ └───────────────────┘ │
│                                                                │
├────────────────────────────────────────────────────────────────┤
│  Tokio Async Runtime                                           │
│  TcpStream · UdpSocket · TLS · DNS · File · Timer              │
└────────────────────────────────────────────────────────────────┘
```

## Why This Matters

This design means Flux achieves Node.js-level I/O concurrency while maintaining:

- **Per-request isolation** — no shared mutable state between requests
- **Full observability** — every I/O operation is a span, automatically, in every language
- **Rust-level I/O performance** — all I/O uses tokio + native async drivers
- **Multi-tenant safety** — one user's slow API call does not block another user's function
- **Language transparency** — users write normal code with normal libraries. Flux is invisible.
- **Deterministic replay** — every I/O call is recorded and can be replayed for debugging
- **Universal protocol support** — any protocol that uses sockets works (HTTP, Redis, Postgres, gRPC, Kafka, MQTT, etc.) with zero driver-specific code

The user writes normal code in their language. Flux handles the rest.
