# Compatibility & Supported Services

Flux operates by executing your code in a Deno V8 isolate. It allows standard web APIs, but intercepts specific I/O calls to write deterministic checkpoints to the execution store.

As Flux is a runtime and not a framework, you can use standard HTTP fetch and generic TCP-based database drivers. However, side-effects that evade TCP socket interception or use non-deterministic host-level APIs will not be replayable.

## What is Supported? ✅

### 1. HTTP and REST Services
- **Native `fetch()`:** You can use the standard web `fetch()` API. Flux intercepts these at the V8 isolate level.
- **HTTP Clients:** Libraries that wrap `fetch()` (such as Axios, ky, or URLFetch) are natively supported.
- All outbound HTTP requests are buffered and checkpointed.

### 2. Databases & TCP Protocols
- **Postgres:** Native Postgres queries over plain TCP or TLS (using `rustls`). Libraries like `postgres.js` and `pg` that communicate over raw sockets are intercepted seamlessly.
- **Redis:** Basic Redis interactions over TCP are supported and intercepted.

### 3. Frameworks
- Standard Deno web frameworks (like **Hono** or **Oak**) work perfectly as they just wrap standard HTTP request/response object lifecycles.

---

## What is NOT Supported? ❌

To ensure that an execution trace is 100% deterministic and can be safely replayed, you must avoid side-effects that Flux cannot intercept:

### 1. Local File System Writes
- Writing to disk using APIs like `Deno.writeFile` or `Deno.writeFileSync` is **not checkpointed**. 
- *Why:* If you replay an execution, the file will be written again, breaking the idempotency guarantee of Flux replays.

### 2. Child Processes
- Spawning child processes using `Deno.Command` or similar `child_process` equivalents.
- *Why:* Child process execution is non-deterministic and the internal I/O of the sub-process cannot be safely intercepted.

### 3. Non-TCP / Unix Sockets
- Communicating via local named pipes or Unix domain sockets (`/var/run/...`). 
- Flux only intercepts native TCP/TLS socket connections.

### 4. Native Addons (N-API / C++)
- Custom C++ plugins or native addons break out of the Deno sandbox. Their network and system calls cannot be intercepted by the Flux runtime.
