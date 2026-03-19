# Compatibility & Supported Services

Flux operates by executing your code in a Deno V8 isolate. It allows standard web APIs, but intercepts specific I/O calls to write deterministic checkpoints to the execution store.

As Flux is a runtime and not a framework, you can use standard HTTP fetch and generic TCP-based database drivers. However, side-effects that evade TCP socket interception or use non-deterministic host-level APIs will not be replayable.

## Productization & Compatibility Strategy 🎯

Flux is designed for a **Minimal Friction** developer experience. Our goal is to support the top 20% of libraries that power 80% of Node.js and Deno backends by providing a deterministic execution layer.

### Compatibility Tiers & Adoption Readiness

Use this table to determine if your current stack is ready for Flux.

| Area | Status | Supported Libraries | Adoption Readiness |
|------|--------|--------------------|-------------------|
| **Web Frameworks** | 🟢 Ready | Hono, standard handlers | **High**: Drop-in for existing Hono apps. |
| | 🟡 Beta | Express, Fastify, Koa | **Medium**: Works with basic middleware. |
| **HTTP Clients** | 🟢 Ready | fetch, axios, undici | **High**: All outbound API calls intercepted. |
| **Databases** | 🟢 Ready | pg (node-postgres), Drizzle | **High**: Robust Postgres support. |
| | 🟡 Beta | postgres.js, ioredis | **Medium**: Basic commands supported. |
| **ORMs** | 🟡 Beta | Kysely, TypeORM | **Medium**: Depends on driver compatibility. |
| | 🔴 Limited | Prisma | **Low**: Technical complexity in interception. |
| **Native Addons** | ❌ None | bcrypt, sqlite3, sharp | **Unsupported**: Escapes V8 sandbox. |

---

## What is Supported? ✅

### 1. Web Standard APIs
The following APIs are natively shimmed and available in the global scope:

- **Fetch API**: `fetch()`, `Request`, `Response`, `Headers`. Intercepted at the isolate level to ensure every outbound request is recorded and replayable.
- **Streams**: `TextEncoder`, `TextDecoder`, `ReadableStream` (buffered).
- **Core Globals**: `URL`, `URLSearchParams`, `FormData`, `DOMException`, `AbortController`.
- **Async & Timers**: `setTimeout`, `setInterval`, `clearTimeout`, `clearInterval`, `queueMicrotask`.
- **Encoding**: `btoa`, `atob`.
- **Crypto**: 
  - `crypto.getRandomValues()`, `crypto.randomUUID()` (patched for determinism).
  - `crypto.subtle.importKey`, `crypto.subtle.verify` (RSASSA-PKCS1-V1_5 with SHA-256).
- **Determinism**: `Date`, `performance.now()`, and `Math.random()` are all patched to ensure re-runs are identical to the original execution.
- **Console**: Fully supported for logging and debugging.

### 2. Node.js Compatibility
We provide shims for the most critical Node.js APIs to ensure popular npm packages function correctly within the Flux sandbox:
- `process.env`: Proxy to system environment variables.
- `process.nextTick()`: Schedules tasks on the microtask queue.
- `process.versions`, `process.platform`, `process.cwd()`.

### 3. Power User / Advanced IO API (`globalThis.Flux`)
For complex integrations or performance-critical IO, Flux provides low-level, deterministic adapters. This is the **Power User** layer that underlies our package compatibility.

- **Postgres (`Flux.postgres`)**:
  - `query()`: Low-level SQL execution with direct checkpointing.
  - `createNodePgPool()`: A compatibility layer for `pg` and `drizzle-orm`.
- **Redis (`Flux.redis`)**:
  - `createClient()`: Mimics the `redis` (node-redis) API.
- **Generic TCP (`Flux.net`)**:
  - `tcpExchange()`: Perform deterministic outbound TCP communication.

---

## What is NOT Supported? ❌

To maintain 100% determinism, Flux must intercept all state-changing interactions. Certain Node.js or OS-level behaviors are currently restricted:

### 1. Native Addons (C++/Rust)
- **Why**: Binary addons (like `bcrypt` or `sqlite3`) execute code outside the V8 sandbox. Since Flux cannot intercept the internal I/O or system calls of these binaries, they break the execution record.
- **Status**: ❌ Not Supported. Use Pure-JS alternatives where available.

### 2. Local File System Writes
- **Why**: Writing to the host disk (e.g., via `fs.writeFile`) escapes the Flux checkpoint system. If an execution is replayed, the file would be written twice, violating the idempotency guarantee.
- **Status**: ❌ Not Supported. Use deterministic cloud storage (S3/R2) via `fetch`.

### 3. Child Processes
- **Why**: Spawning external processes (`Deno.Command`) introduces non-deterministic behavior. Flux cannot safely capture or replay the interactions of a sub-process.
- **Status**: ❌ Not Supported.

### 4. Raw Unix Sockets
- **Why**: Flux intercepts traffic at the TCP/TLS layer. Local named pipes or Unix domain sockets (often used for local DB connections) escape this interception.
- **Status**: ❌ Not Supported. Use TCP connections instead.
