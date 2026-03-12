# Single Binary Architecture — What Needs to Change

**Date:** March 13, 2026  
**Context:** Architectural decision — Flux becomes a single binary, single port, in-process architecture. No more HTTP between services.

---

## The Decision

Flux is a **single binary**. One process, one port (:4000), all five modules (Gateway, Runtime, API, Data Engine, Queue) running in-process. No HTTP between them.

**Why:**
- Every module is stateless. Postgres holds all state. There's no reason to run them as separate processes.
- Inter-service HTTP adds ~0.3ms per hop. A typical request has 2-3 hops (Gateway → Runtime → Data Engine → Runtime → Gateway). That's 1-1.5ms of pure overhead on a function that does 2ms of real work. Eliminating it is a 30-40% latency improvement.
- Scaling means running more copies of the same binary behind a load balancer. You never need to scale Runtime independently of Gateway — they're both stateless against the same Postgres.
- Operational simplicity: one Docker image, one health check, one port to expose, no service discovery, no `RUNTIME_URL` / `API_URL` / `QUEUE_URL` env vars.

---

## Current State (what exists today)

Each service is a separate binary with its own `main.rs`, its own `TcpListener`, its own port:

```
gateway/src/main.rs      → axum::serve on :4000
runtime/src/main.rs      → axum::serve on :8083  
api/src/main.rs          → axum::serve on :8080
data-engine/src/main.rs  → axum::serve on :8082
queue/src/main.rs        → axum::serve on :8084
```

Services communicate over HTTP:
- **Gateway → Runtime:** `http_client.post(format!("{}/execute", state.runtime_url))` (see `gateway/src/forward/mod.rs:27`)
- **Runtime → API:** `http_client.get(format!("{}/internal/bundle", state.api_url))` (see `runtime/src/execute/bundle.rs:72`)
- **Runtime → API:** `http_client.post(format!("{}/internal/logs", state.api_url))` (see `runtime/src/execute/handler.rs:57`)
- **Runtime → Queue:** `queue_url` used for job pushes (see `runtime/src/execute/runner.rs:101`)
- **Runtime → API:** secrets fetched over HTTP (see `runtime/src/secrets/client.rs:90`)

---

## Target State

### 1. New `server` crate

Add a new workspace member:

```toml
# Root Cargo.toml
[workspace]
members = [
    "api",
    "runtime",
    "cli",
    "gateway",
    "queue",
    "data-engine",
    "shared/job_contract",
    "server",           # ← NEW: single binary that composes all modules
]
```

The `server` crate depends on all 5 service crates as **libraries** (not binaries):

```toml
# server/Cargo.toml
[package]
name = "flux-server"

[dependencies]
gateway     = { path = "../gateway" }
runtime     = { path = "../runtime" }
api         = { path = "../api" }
data-engine = { path = "../data-engine" }
queue       = { path = "../queue" }
axum        = "..."
tokio       = { version = "...", features = ["full"] }
sqlx        = "..."
```

### 2. Each service crate exposes a `pub fn router()` (library, not binary)

Each crate keeps its `main.rs` for now (backward compat during migration), but the real entry point becomes a library function:

```rust
// gateway/src/lib.rs (NEW)
pub fn router(state: Arc<GatewayState>) -> Router {
    router::create_router(state)  // already exists in gateway/src/router.rs
}

// api/src/lib.rs (NEW)
pub fn router(state: AppState) -> Router {
    create_app(state)  // already exists as pub fn create_app() in api/src/main.rs
}

// Same pattern for runtime, data-engine, queue
```

### 3. server/src/main.rs — the single binary

```rust
// server/src/main.rs
use std::sync::Arc;
use tokio::net::TcpListener;
use sqlx::postgres::PgPoolOptions;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    
    let config = load_config();  // one config, not 5 separate ones
    
    // ONE shared connection pool
    let db = PgPoolOptions::new()
        .max_connections(config.max_db_connections)  // e.g. 50
        .connect(&config.database_url)
        .await?;

    // Build module instances — in-process, shared memory
    let data_engine = Arc::new(data_engine::DataEngine::new(db.clone()));
    let queue_engine = Arc::new(queue::QueueEngine::new(db.clone()));
    let runtime = Arc::new(runtime::Runtime::new(
        db.clone(),
        data_engine.clone(),   // direct Arc reference, not a URL
        queue_engine.clone(),  // direct Arc reference, not a URL
    ));
    let gateway = gateway::Gateway::new(
        db.clone(),
        runtime.clone(),       // direct Arc reference, not a URL
    );

    // ONE router, ONE port
    let app = Router::new()
        .merge(gateway.public_routes())         // POST /<function_name>
        .merge(gateway.health_routes())         // GET /health
        .nest("/internal", api::router(db.clone()))  // CLI queries
        .layer(cors_layer())
        .layer(TraceLayer::new_for_http());

    let addr = format!("0.0.0.0:{}", config.port);  // default: 4000
    let listener = TcpListener::bind(&addr).await?;
    tracing::info!("Flux running on http://localhost:{}", config.port);
    axum::serve(listener, app).await?;
    
    Ok(())
}
```

### 4. Replace HTTP dispatch with trait-based in-process dispatch

The key refactor — this is where the latency win comes from.

**Gateway → Runtime (current: HTTP)**

```rust
// gateway/src/forward/mod.rs (CURRENT)
let url = format!("{}/execute", state.runtime_url);
let res = state.http_client.post(&url)
    .json(&execute_request)
    .send().await?;
```

**Gateway → Runtime (new: in-process)**

```rust
// Define a trait in shared/
#[async_trait]
pub trait RuntimeDispatch: Send + Sync {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteResponse>;
}

// In-process implementation (used by single binary)
pub struct InProcessRuntime {
    engine: Arc<runtime::IsolatePool>,
    // ... other runtime internals
}

#[async_trait]
impl RuntimeDispatch for InProcessRuntime {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteResponse> {
        self.engine.execute(req).await  // direct function call, no serialization
    }
}
```

Same pattern for:
- **Runtime → Data Engine:** replace `http_client.post(data_engine_url)` with `data_engine.query(req).await`
- **Runtime → Queue:** replace `http_client.post(queue_url)` with `queue_engine.push(job).await`  
- **Runtime → API (bundle fetch):** replace `http_client.get(api_url/internal/bundle)` with `api.get_bundle(function_id).await`
- **Runtime → API (secrets):** replace `http_client.get(api_url/internal/secrets)` with `api.get_secrets(keys).await`
- **Runtime → API (logs):** replace `http_client.post(api_url/internal/logs)` with `api.write_logs(entries).await`

---

## What to Change — File by File

### Phase A: Extract library interfaces (~2 days)

Each service crate gets a `lib.rs` that exposes its router and core types:

| File | Change |
|------|--------|
| `gateway/src/lib.rs` | NEW — re-export `router::create_router`, `state::GatewayState`, `snapshot::GatewaySnapshot` |
| `runtime/src/lib.rs` | NEW — re-export `AppState`, router, `IsolatePool`, `execute` logic |
| `api/src/lib.rs` | NEW — re-export `create_app`, `AppState`, internal route handlers |
| `data-engine/src/lib.rs` | NEW — re-export router, `DataEngine` core, query/mutate functions |
| `queue/src/lib.rs` | NEW — re-export router, `QueueEngine`, push/poll functions |

### Phase B: Define dispatch traits (~1 day)

| File | Change |
|------|--------|
| `shared/job_contract/src/dispatch.rs` | NEW — `RuntimeDispatch`, `DataEngineDispatch`, `QueueDispatch` traits |

### Phase C: Refactor inter-service calls (~3-4 days)

This is the main work. Replace every `http_client.post(url)` between services with a trait call:

| File | Current (HTTP) | New (trait call) |
|------|----------------|------------------|
| `gateway/src/forward/mod.rs` | `http_client.post(runtime_url/execute)` | `self.runtime.execute(req).await` |
| `gateway/src/state.rs` | `runtime_url: String, http_client: Client` | `runtime: Arc<dyn RuntimeDispatch>` |
| `runtime/src/execute/bundle.rs` | `http_client.get(api_url/internal/bundle)` | `self.api.get_bundle(id).await` |
| `runtime/src/execute/handler.rs` | `http_client.post(api_url/internal/logs)` | `self.api.write_logs(entries).await` |
| `runtime/src/execute/runner.rs` | `queue_url` for job pushes | `self.queue.push(job).await` |
| `runtime/src/secrets/client.rs` | `http_client.get(api_url/internal/secrets)` | `self.api.get_secrets(keys).await` |
| `runtime/src/main.rs` | `api_url: String, queue_url: String` | `api: Arc<dyn ApiDispatch>, queue: Arc<dyn QueueDispatch>` |

### Phase D: Build the server crate (~1 day)

| File | Change |
|------|--------|
| `server/Cargo.toml` | NEW — depends on all 5 crates |
| `server/src/main.rs` | NEW — compose all modules, one router, one port |
| `server/src/config.rs` | NEW — unified config (one `DATABASE_URL`, one `PORT`, done) |

### Phase E: Update CLI and Docker (~1 day)

| File | Change |
|------|--------|
| `cli/src/dev.rs` | Spawn `flux-server` (one process) instead of 5 processes |
| `docker-compose.yml` | Two services: `postgres` + `flux` (was 6 services) |
| `Dockerfile` | Build `flux-server` binary |
| All `*/Dockerfile` | Can be deleted (individual service Dockerfiles) |

---

## What NOT to Change

- **Each crate stays as a workspace member.** Don't merge them into one crate. Module boundaries are good for code organization.
- **Each crate keeps its `main.rs` temporarily.** Useful for development/testing of individual modules. Remove later when single binary is stable.
- **Route snapshot (LISTEN/NOTIFY).** Still works — the gateway module still reads routes from Postgres and caches in-memory. The NOTIFY channel still fires on route changes. Nothing changes here.
- **The Deno isolate pool.** Runtime internals are untouched. It's just called directly instead of through HTTP.

---

## Config Simplification

**Current: 5 service configs, 12+ env vars**
```
GATEWAY_PORT=4000
RUNTIME_URL=http://localhost:8083
RUNTIME_PORT=8083
API_PORT=8080
API_URL=http://localhost:8080
DATA_ENGINE_PORT=8082
DATA_ENGINE_URL=http://localhost:8082
QUEUE_PORT=8084
QUEUE_URL=http://localhost:8084
DATABASE_URL=postgres://...
INTERNAL_SERVICE_TOKEN=...
LOCAL_MODE=true
```

**New: 1 config**
```
PORT=4000
DATABASE_URL=postgres://...
```

That's it. No inter-service URLs (they're in-process). No service tokens (there's no network boundary between modules). Local mode is detected by the CLI.

---

## Scaling Story

```
                    Load Balancer (:443)
                    ┌──────┼──────┐
                    ▼      ▼      ▼
              flux-server  flux-server  flux-server
              (full stack) (full stack) (full stack)
                    └──────┼──────┘
                           ▼
                    Postgres (shared)
```

- Every instance is identical. No "I need 3 runtimes but only 1 gateway."
- Horizontal scaling: `replicas: N` in Kubernetes, or N containers in Docker Swarm / ECS.
- Connection pooling: each instance gets `max_connections / N` from Postgres.
- Session affinity not required — everything is stateless.

---

## Timeline Estimate

| Phase | Work | Estimate |
|-------|------|----------|
| A: Extract lib interfaces | Add `lib.rs` to each crate | 2 days |
| B: Define dispatch traits | 3-4 traits in shared crate | 1 day |
| C: Refactor HTTP → in-process | Main refactor — ~6 files | 3-4 days |
| D: Build server crate | `server/src/main.rs` + config | 1 day |
| E: CLI + Docker | `dev.rs`, Dockerfile, compose | 1 day |
| **Total** | | **~8-10 days** |

Phase A and B can be done without breaking anything. Phase C is where the HTTP calls get replaced — this is the riskiest part, do it one service boundary at a time (Gateway→Runtime first, then Runtime→DataEngine, then Runtime→Queue, then Runtime→API).

---

*Read framework.md §4 (Architecture) for the updated design. This doc is the implementation plan.*
