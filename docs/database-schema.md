# Database Schema Architecture

> **Internal architecture doc.** Canonical reference for the Flux Postgres
> schema layout, table ownership, and naming conventions.

---

## Two schemas, two concerns

```
Postgres
  ├── flux schema     — Flux system tables (platform internals)
  └── public schema   — User application tables (their data)
```

User function code can only see `public`. System services set
`search_path = flux, public` so they resolve system tables first.

**Critical rule:** User functions never reference `flux.*` tables directly.
All system interaction goes through `ctx.*` methods which call the appropriate
service API.

---

## Table naming convention

Tables are prefixed by the service that **owns** them (the sole writer):

```
flux.<service>_<table>
```

### Shared reference tables (no prefix — read by all, written only by API)

| Table | Description |
|---|---|
| `flux.projects` | Project registry |
| `flux.secrets` | Encrypted project secrets |

### API service tables

| Table | Description |
|---|---|
| `flux.api_functions` | Registered functions |
| `flux.api_deployments` | Deployment history |
| `flux.api_routes` | Route → function mapping |
| `flux.api_api_keys` | Hashed API keys |

### Gateway tables (append-only)

| Table | Description |
|---|---|
| `flux.gateway_trace_requests` | Trace root per request |

### Runtime tables (append-only)

| Table | Description |
|---|---|
| `flux.runtime_function_logs` | Structured function logs |
| `flux.runtime_execution_records` | Full execution record (spans, mutations, calls) |

### Queue tables

| Table | Description |
|---|---|
| `flux.queue_jobs` | Pending / in-flight async jobs |
| `flux.queue_dead_letter` | Failed jobs past retry limit |

---

## Ownership matrix

| Table | Writer | Readers |
|---|---|---|
| `flux.projects` | API | Gateway, Runtime, Queue |
| `flux.secrets` | API | Runtime |
| `flux.api_functions` | API | Runtime, Gateway |
| `flux.api_deployments` | API | Runtime |
| `flux.api_routes` | API | Gateway (snapshot) |
| `flux.api_api_keys` | API | Gateway (auth check) |
| `flux.gateway_trace_requests` | Gateway | API (`flux trace`) |
| `flux.runtime_function_logs` | Runtime | API (`flux logs`) |
| `flux.runtime_execution_records` | Runtime | API (`flux trace`, `flux why`) |
| `flux.queue_jobs` | Queue | Queue |
| `flux.queue_dead_letter` | Queue | API |

**Cross-service read rule:**
- Transactional data → go through the owning service's API endpoint
- Append-only observability data → direct `SELECT` is fine (immutable)

**Cross-service write rule:**
- Never. Call the owning service's internal API instead.

---

## Why direct writes are forbidden for transactional tables

If two services write the same table:
- A bug in one corrupts data the other depends on
- Schema changes require coordinating multiple services
- "Who changed this row?" has more than one answer
- Audit trails break

**Example — wrong:**
```rust
// Runtime writing directly to API's table ❌
sqlx::query("UPDATE flux.api_functions SET status = 'active' WHERE id = $1")
    .bind(function_id)
    .execute(&pool).await?;
```

**Example — correct:**
```rust
// Runtime calling the API service ✅
http_client
    .post(format!("{}/internal/functions/{}/status", api_url, function_id))
    .json(&json!({ "status": "active" }))
    .send().await?;
```

---

## Append-only exception

`flux.gateway_trace_requests` and `flux.runtime_*` tables are written directly
by their owning service because:

1. **Append-only** — only INSERTs, never UPDATE or DELETE
2. **Hot path** — routing through the API would add latency on every request
3. **No business logic** — pure telemetry, no state machine

These tables also use `ON CONFLICT DO NOTHING` so duplicate writes are safe.

---

## User application data (`public` schema)

```
public.*   — created by `flux db push` from the user's schemas/ directory
```

- Flux never reads or writes `public.*` directly
- All access goes through the Data Engine (`ctx.db.*`)
- The Data Engine intercepts every write and records before/after state
- `flux db push` applies `schemas/*.sql` to `public` only

---

## search_path convention

| Service | search_path | Reason |
|---|---|---|
| Gateway, API, Queue | `flux, public` | Resolves system tables first |
| Runtime (system queries) | `flux, public` | Bundle fetch, secrets |
| Runtime (user function queries) | `public` | Isolates user code from flux.* |
| Data Engine | `public` | All user queries are application data |

Set on connection open:
```rust
sqlx::query("SET search_path = flux, public").execute(conn).await?;
```

---

## Migration ownership

Migration files are prefixed by the service they affect:

```
api/migrations/
  20240307000001_init.sql                    — shared bootstrap
  20240307000002_api_projects.sql            — api tables
  20240307000003_api_functions_routes.sql    — api tables
  20260312000029_route_notify_trigger.sql    — gateway trigger (owned by gateway)
```

The `route_change_notify` trigger in `20260312000029` is a gateway concern
even though it lives in the API migrations folder — it fires on `flux.api_routes`
which the API owns.

---

*For service-specific table details see: [gateway.md](gateway.md),
[runtime.md](runtime.md), [api.md](api.md), [data-engine.md](data-engine.md).*
