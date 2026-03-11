# Data Engine

The Data Engine is Flowbase's internal data-access tier — a Rust/Axum microservice that sits between the public-facing API and the Postgres database. No user traffic reaches it directly; everything flows through the API or Gateway, authenticated via a shared service token.

---

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [Startup & Background Workers](#startup--background-workers)
4. [API Surface](#api-surface)
5. [Request Lifecycle](#request-lifecycle)
6. [Core Modules](#core-modules)
   - [Auth Context](#auth-context)
   - [Service Authentication Middleware](#service-authentication-middleware)
   - [DB Router](#db-router)
   - [Policy Engine](#policy-engine)
   - [Query Compiler](#query-compiler)
   - [Executor](#executor)
   - [Cache Layer](#cache-layer)
   - [Query Guard](#query-guard)
   - [Transform Engine](#transform-engine)
   - [File Engine](#file-engine)
   - [Hook Engine](#hook-engine)
   - [Events System](#events-system)
   - [Workflow Engine](#workflow-engine)
   - [Cron Worker](#cron-worker)
7. [Database Schema](#database-schema)
8. [Deterministic Execution & Replay](#deterministic-execution--replay)
9. [Configuration Reference](#configuration-reference)
10. [Deployment Notes](#deployment-notes)
11. [Architectural Gaps & Improvements](#architectural-gaps--improvements)

---

## Overview

```
  Client / SDK
       │
       ▼
  [ API Service ]  ──x-service-token──▶  [ Data Engine ]
       │                                        │
       │ (auth forwarded in x-* headers)        ▼
       │                               [ Postgres / Neon ]
       │
       ▼
  [ Gateway ] ────x-service-token────▶  [ Data Engine ]
```

The Data Engine is responsible for:

- **Secure multi-tenant data access** — all queries are scoped to a `(tenant, project, database)` triple.
- **Row/column level security** — policy evaluation before every mutation or fetch.
- **Schema management** — create/alter/drop Postgres schemas, tables, and relationships.
- **Lifecycle hooks** — invoke serverless functions before/after every table mutation.
- **Event delivery** — persist and fan-out events to webhooks, functions, and queues.
- **Workflow execution** — step-advance workflow executions triggered by events.
- **Cron scheduling** — fire cron jobs whose schedule has become due.
- **File upload/download** — presigned S3 URLs with per-table access control.

---

## Architecture

```
data-engine/src/
├── main.rs             Entry point; wires pool → AppState → workers → server
├── config.rs           Env-var driven Config struct
├── state.rs            AppState — shared caches, pool, file engine, query guard
│
├── api/
│   ├── routes.rs       Axum router — all HTTP routes
│   ├── handlers/       One file per resource group (query, tables, policies, …)
│   └── middleware/     service_auth.rs — token gating for every request
│
├── engine/
│   ├── auth_context.rs Extracts tenant/user context from x-* headers
│   └── error.rs        EngineError enum → HTTP status codes
│
├── db/
│   └── connection.rs   sqlx PgPool initialisation
│
├── router/
│   └── db_router.rs    Schema naming convention + schema existence checks
│
├── compiler/
│   ├── query_compiler.rs  Compiles QueryRequest → SQL template + bind params
│   └── relational.rs      Nested selector parsing, lateral CTEs, batched plans
│
├── executor/
│   ├── db_executor.rs  Executes a single CompiledQuery inside a transaction
│   └── batched.rs      Executes a BatchedPlan (multi-round-trip deep nesting)
│
├── cache/
│   └── mod.rs          Two-layer Moka cache (schema + plan)
│
├── query_guard.rs       Complexity scoring + timeout wrapper
├── policy/
│   └── engine.rs        Role-based policy evaluation with in-process cache
│
├── transform/
│   └── engine.rs        Column metadata loading + file-URL post-processing
│
├── file_engine/
│   └── engine.rs        S3 presigned PUT/GET URL generation
│
├── hooks/
│   └── engine.rs        Before/after lifecycle hook invocation
│
├── events/
│   ├── emitter.rs       Writes events to the DB after mutations
│   ├── dispatcher.rs    Dispatches one event to one subscription target
│   └── worker.rs        Background loop: poll undelivered events, retry with backoff
│
├── workflow/
│   └── engine.rs        Trigger workflows + background step-advancement loop
│
└── cron/
    └── worker.rs        Background loop: fire due cron jobs every 30 s
```

---

## Startup & Background Workers

`main.rs` performs startup in order:

1. Load config from environment variables.
2. Open a `PgPool` (sqlx connection pool) to Postgres.
3. Build `AppState` — initialises caches, file engine, query guard.
4. Spawn three independent background tasks, each sharing the pool:

| Worker | Interval | Responsibility |
|---|---|---|
| **Events worker** | Poll loop | Deliver `pending` event records to subscriptions; exponential retry |
| **Workflow engine** | 2 s tick | Claim `running` executions, advance to next step via runtime |
| **Cron worker** | 30 s tick | Fire cron jobs whose `next_run_at ≤ now()` |

5. Start Axum HTTP server on `0.0.0.0:{PORT}`.

All workers use `FOR UPDATE SKIP LOCKED` so multiple replicas can run safely in parallel without duplicate dispatch.

---

## API Surface

All routes are prefixed with the service's base URL. Every route except `/health` and `/version` requires the `x-service-token` header.

### Data API

| Method | Path | Description |
|---|---|---|
| `POST` | `/db/query` | Execute a query (select / insert / update / delete) |

### Schema Management

| Method | Path | Description |
|---|---|---|
| `GET` | `/db/schema` | Introspect full schema graph (tables, columns, relationships) |
| `POST` | `/db/databases` | Create a new project database (Postgres schema) |
| `GET` | `/db/databases` | List databases for tenant+project |
| `DELETE` | `/db/databases/{name}` | Drop a database (CASCADE) |
| `POST` | `/db/tables` | Create a table with column definitions |
| `GET` | `/db/tables/{database}` | List tables in a database |
| `DELETE` | `/db/tables/{database}/{table}` | Drop a table |

### Access Control

| Method | Path | Description |
|---|---|---|
| `GET` | `/db/policies` | List all policies |
| `POST` | `/db/policies` | Create a policy (role + table + operation + conditions) |
| `DELETE` | `/db/policies/{id}` | Delete a policy |

### Lifecycle Hooks

| Method | Path | Description |
|---|---|---|
| `GET` | `/db/hooks` | List hooks |
| `POST` | `/db/hooks` | Register a hook (before/after insert/update/delete) |
| `PATCH` | `/db/hooks/{id}` | Update a hook |
| `DELETE` | `/db/hooks/{id}` | Delete a hook |

### Relationships

| Method | Path | Description |
|---|---|---|
| `GET` | `/db/relationships` | List relationships |
| `POST` | `/db/relationships` | Define a foreign-key relationship |
| `DELETE` | `/db/relationships/{id}` | Remove a relationship |

### Event Subscriptions

| Method | Path | Description |
|---|---|---|
| `GET` | `/db/subscriptions` | List event subscriptions |
| `POST` | `/db/subscriptions` | Create subscription (webhook / function / queue) |
| `PATCH` | `/db/subscriptions/{id}` | Update a subscription |
| `DELETE` | `/db/subscriptions/{id}` | Delete a subscription |

### Workflows

| Method | Path | Description |
|---|---|---|
| `GET` | `/db/workflows` | List workflows |
| `POST` | `/db/workflows` | Create a workflow |
| `DELETE` | `/db/workflows/{id}` | Delete a workflow |
| `POST` | `/db/workflows/{id}/steps` | Add a step to a workflow |

### Cron

| Method | Path | Description |
|---|---|---|
| `GET` | `/db/cron` | List cron jobs |
| `POST` | `/db/cron` | Create a cron job |
| `PATCH` | `/db/cron/{id}` | Update a cron job |
| `DELETE` | `/db/cron/{id}` | Delete a cron job |
| `POST` | `/db/cron/{id}/trigger` | Manually trigger a cron job immediately |

### Files

| Method | Path | Description |
|---|---|---|
| `POST` | `/files/upload-url` | Get presigned PUT URL for direct browser upload |
| `POST` | `/files/download-url` | Get presigned GET URL for a stored file key |

### System

| Method | Path | Description |
|---|---|---|
| `GET` | `/health` | Returns `{ "status": "ok" }` — no auth required |
| `GET` | `/version` | Returns service name + git SHA + build time |

Request body size is capped at **1 MB**.

---

## Request Lifecycle

The following describes the full path for `POST /db/query`:

```
POST /db/query
  │
  ├─ 1. Middleware: service token check (x-service-token)
  │
  ├─ 2. AuthContext ← x-tenant-id, x-project-id, x-tenant-slug, x-project-slug,
  │                   x-user-id, x-user-role
  │
  ├─ 3. Schema name → "t_{tenant_slug}_{project_slug}_{db_name}"
  │
  ├─ 4. QueryGuard:
  │     ├─ Complexity score check  → HTTP 400 if over ceiling
  │     └─ Nesting depth check     → HTTP 400 if too deep
  │
  ├─ 5. Schema existence check (information_schema)  → HTTP 404 if missing
  │
  ├─ 6. Table existence check                        → HTTP 404 if missing
  │
  ├─ 7. Policy Engine (read-through in-process cache):
  │     ├─ Load policy for (tenant, project, table, role, operation)
  │     ├─ Exact match first, then wildcard '*' operation
  │     └─ HTTP 403 if no matching policy
  │
  ├─ 8. Schema cache (L1, Moka TTL 60 s):
  │     ├─ Hit  → (col_meta, relationships) from memory
  │     └─ Miss → load from fluxbase_internal.column_metadata + relationships
  │
  ├─ 9. [mutations only] Before hook:
  │     ├─ Load enabled hooks for (table, before_<op>) event
  │     ├─ POST to runtime /internal/execute for each hook function
  │     └─ Non-2xx response aborts the operation (HTTP 500)
  │
  ├─ 10. Compiler:
  │      ├─ Plan cache (L2, Moka TTL 300 s) — SELECT only:
  │      │   ├─ Hit  → rebuild bind params from request; skip full compile
  │      │   └─ Miss → QueryCompiler::compile → SQL template + params
  │      └─ Nested depth ≥ BATCH_DEPTH_THRESHOLD → BatchedPlan path
  │
  ├─ 11. Executor:
  │      ├─ Single path  → db_executor::execute (transaction + json_agg)
  │      └─ Batched path → batched::execute (root query + N child fetches merged in Rust)
  │
  ├─ 12. Transform Engine:
  │      ├─ File columns → replace S3 key with presigned GET URL (private)
  │      │  or public CDN URL
  │      └─ No-op when no file columns or file engine not configured
  │
  ├─ 13. [mutations only] After hook (non-fatal — data already committed):
  │      └─ POST to runtime /internal/execute for each after_<op> hook
  │
  ├─ 14. Events:
  │      └─ EventEmitter writes event record to DB for async delivery
  │
  └─ 15. Return JSON response
```

---

## Core Modules

### Auth Context

`engine/auth_context.rs`

Extracts caller identity from headers injected by the API or Gateway. No JWT verification happens inside the Data Engine — the API validates the token and forwards trusted headers over the internal network.

**Headers consumed:**

| Header | Type | Description |
|---|---|---|
| `x-tenant-id` | UUID | Required |
| `x-project-id` | UUID | Required |
| `x-tenant-slug` | string | Falls back to deriving from UUID |
| `x-project-slug` | string | Falls back to deriving from UUID |
| `x-user-id` | string | Firebase UID or equivalent |
| `x-user-role` | string | `anon` \| `authenticated` \| `admin` \| `service` (defaults to `anon`) |

---

### Service Authentication Middleware

`api/middleware/service_auth.rs`

Every request must carry `x-service-token` matching the `INTERNAL_SERVICE_TOKEN` env var. `/health` and `/version` are exempt so load balancer health checks work without credentials.

If the token is missing or wrong, returns:

```json
HTTP 401
{ "error": "unauthorized: missing or invalid x-service-token" }
```

---

### DB Router

`router/db_router.rs`

Maps a logical `(tenant_slug, project_slug, db_name)` triple to a Postgres schema name.

**Convention:** `t_{tenant_slug}_{project_slug}_{db_name}`

Example: tenant `acme`, project `auth`, database `main` → `t_acme_auth_main`

Responsibilities:
- Generate and validate schema names (SQL injection prevention via identifier validation).
- `CREATE SCHEMA IF NOT EXISTS` for database creation.
- `DROP SCHEMA CASCADE` for database deletion.
- Assert schema/table existence before query execution.

**`validate_identifier` contract:** Only characters matching `[a-z0-9_]+` (after lowercasing) are permitted in schema and table name components. Any other character — including quotes, semicolons, spaces, or Unicode — is rejected with `HTTP 400` before SQL generation. This prevents all schema-name-based injection paths regardless of how the slug was constructed.

---

### Policy Engine

`policy/engine.rs`

Implements role-based access control with optional column-level and row-level restrictions.

**Policy evaluation order:**
1. Exact match: `(role, table, operation)`
2. Wildcard operation: `(role, table, '*')`
3. If neither exists → `HTTP 403 AccessDenied`

**PolicyResult** fields:
- `allowed_columns` — columns the role may read/write. Empty = all columns permitted.
- `row_condition_sql` — parameterised SQL fragment added to WHERE clause for row-level filtering.
- `row_condition_params` — bind values substituted from `$auth.*` template variables.

**Template variables in `row_condition`:**

| Variable | Substituted with |
|---|---|
| `$auth.uid` | `x-user-id` header value |
| `$auth.role` | `x-user-role` header value |
| `$auth.tenant_id` | `x-tenant-id` header value |
| `$auth.project_id` | `x-project-id` header value |

**Caching:** Policies are cached in an in-process `RwLock<HashMap>`. Cache is invalidated on policy writes (`invalidate_policy_cache`) and on schema changes (`invalidate_tenant_schema`).

---

### Query Compiler

`compiler/query_compiler.rs` + `compiler/relational.rs`

Compiles a `QueryRequest` (JSON API request) into a parameterised SQL string.

**Supported operations:** `select`, `insert`, `update`, `delete`

**QueryRequest shape:**

```json
{
  "database": "main",
  "table": "users",
  "operation": "select",
  "columns": ["id", "name", "posts(id, title)"],
  "filters": [
    { "column": "active", "op": "eq", "value": true }
  ],
  "limit": 20,
  "offset": 0,
  "data": {}
}
```

**Filter operators:** `eq`, `neq`, `gt`, `gte`, `lt`, `lte`, `like`, `ilike`, `is_null`, `not_null`

**Nested selectors** — columns like `posts(id, title)` are expanded to:
- `LATERAL (SELECT ... FROM schema.posts WHERE ...)` subqueries for shallow nesting.
- `BatchedPlan` (separate round-trips merged in Rust) when depth ≥ `BATCH_DEPTH_THRESHOLD`.

**Computed columns** — expressions stored in `column_metadata` are injected directly into the SELECT list as `expr AS "name"` at compile time — no post-processing needed.

**Policy enforcement injected at compile time:**
- Column filtering: only allowed columns appear in SELECT / INSERT / UPDATE.
- Row condition: appended to the WHERE clause with bind parameters.

---

### Executor

`executor/db_executor.rs` + `executor/batched.rs`

**Single path (`db_executor`):**

1. Opens an explicit Postgres transaction.
2. Sets `search_path` for the transaction: `SET LOCAL search_path = {tenant_schema}, public` — enforces tenant boundary at the DB level so a missing schema prefix in any SQL fragment cannot resolve to the wrong tenant's table. *(planned — see [Gap 5](#gap-5--transaction-scoped-search_path-enforcement-hardening))*
3. Wraps the compiled SQL in `SELECT COALESCE(json_agg(row_to_json("_r")), '[]') FROM (...) AS "_r"` so the result is always a JSON array.
4. Binds all parameters via `PgArguments` (type-safe, never string-interpolated).
5. Commits the transaction.

**Batched path (`batched`):**

Used when the selector nest depth exceeds the threshold. Executes:
1. Root query (flat columns only).
2. One query per nesting level, collecting parent IDs.
3. Merges child rows into parent objects in Rust.

Avoids deep lateral subquery expansion that can stress the Postgres planner.

---

### Cache Layer

`cache/mod.rs`

Two-level Moka LRU cache, both with automatic TTL eviction and explicit invalidation.

| Layer | Key format | TTL | Stores | Invalidated on |
|---|---|---|---|---|
| **L1 Schema** | `{tenant_id}:{project_id}:{schema}:{table}` | 60 s | `col_meta` + `relationships` | DDL mutations (CREATE/ALTER/DROP table) |
| **L2 Plan** | Struct key: tenant, project, schema, table, columns, filters shape, policy fingerprint | 300 s | Compiled SQL template + `has_file_cols` + `is_batched` flags | Same as L1 |

**On a SELECT plan cache hit,** the handler rebuilds bind parameters from the request (O(filters) walk) and skips the full compiler pipeline. This eliminates both DB round-trips and CPU-intensive SQL generation on hot paths.

---

### Query Guard

`query_guard.rs`

Enforces complexity and depth limits **before any database work**, returning `HTTP 400` immediately on violation.

**Complexity scoring model:**

| Component | Score |
|---|---|
| Each `filters` clause | +2 |
| Nested selector, depth 1 | +10 |
| Nested selector, depth N | +10 × 2^(N−1) |

**Examples:**

| Query | Score |
|---|---|
| `SELECT *` | 0 |
| `SELECT * WHERE a=1 AND b=2` | 4 |
| `users → posts(id)` | 10 |
| `users → posts → comments` | 30 |
| `users → posts → comments → likes` | 70 |
| depth 5 chain | 150 |

**Timeout:** All execution is wrapped in `tokio::time::timeout`. Exceeding the configured limit returns `HTTP 408`.

**Defaults:**

| Setting | Default |
|---|---|
| `MAX_QUERY_COMPLEXITY` | 1000 |
| `QUERY_TIMEOUT_MS` | 30 000 |
| `MAX_NEST_DEPTH` | 6 |

---

### Transform Engine

`transform/engine.rs`

Post-query processing applied to SELECT results.

1. **File columns (`fb_type = "file"`):** Replaces stored S3 object keys with presigned GET URLs (`private` visibility) or public CDN URLs (`public` visibility). Skipped when the file engine is not configured.
2. **Computed columns:** Handled entirely at compile time (SQL expressions), so the transform engine does not evaluate them.

Column metadata is loaded from `fluxbase_internal.column_metadata` and cached in the L1 schema cache.

---

### File Engine

`file_engine/engine.rs`

Wraps the AWS SDK S3 client for presigned URL generation.

**S3 object key convention:**
```
{tenant_slug}/{project_slug}/{schema}/{table}/{row_id}/{column}/{uuid}.{ext}
```

**Operations:**
- `upload_url(key, content_type, expires_in)` — presigned PUT (default 15 min TTL).
- `download_url(key, expires_in)` — presigned GET (default 1 hour TTL).

Enabled only when `FILES_BUCKET` (or legacy `S3_BUCKET`) is set. If unset, the file engine is `None` and file-column processing is silently skipped.

Supports custom S3-compatible endpoints (MinIO, Localstack) via `S3_ENDPOINT` + path-style addressing.

---

### Hook Engine

`hooks/engine.rs`

Calls serverless functions registered against table lifecycle events.

**Supported events:** `before_insert`, `after_insert`, `before_update`, `after_update`, `before_delete`, `after_delete`

**Before hooks:**
- Called before the mutation executes.
- A non-2xx response from the runtime **aborts** the operation (`HTTP 500`).
- Useful for validation, enrichment, or conditional rejection.

**After hooks:**
- Called after the mutation commits.
- Failures are **non-fatal** — logged as warnings, not propagated to the caller.
- Useful for notifications, side effects, or downstream sync.

Hook invocations reach the Runtime service via `POST {RUNTIME_URL}/internal/execute`.

> **Gap (not yet implemented):** Hook invocations do not currently forward `x-request-id`, `x-parent-span-id`, or `code_sha`. Without these, the trace chain breaks at the data engine → hook runtime boundary. Traces cannot be linked:
> ```
> gateway → runtime → data engine mutation → hook runtime
> ```
> These headers must be injected into every hook dispatch call.

---

### Events System

`events/emitter.rs` + `events/dispatcher.rs` + `events/worker.rs`

**Emit phase** (synchronous, inline with mutation):
- `EventEmitter` writes an event record to `fluxbase_internal.events` with status `pending`.
- Payload includes the mutated row(s) and event type (e.g. `users.insert`).

**Delivery phase** (asynchronous, background worker):
- Worker polls `fluxbase_internal.events` for `pending` records.
- For each event, loads matching subscriptions from `fluxbase_internal.event_subscriptions`.
- Calls `dispatcher::dispatch` per subscription.

**Dispatch targets:**

| Target type | Mechanism |
|---|---|
| `webhook` | `POST` to configured URL with optional HMAC-SHA256 signature + custom headers |
| `function` | `POST {RUNTIME_URL}/internal/execute` with `function_id` + payload |
| `queue_job` | Insert job into the queue service DB |

**Retry:** Failed deliveries are tracked in `fluxbase_internal.event_deliveries` with exponential backoff. The worker skips events that have exhausted their retry budget.

**HMAC signature:** Set `secret` in the webhook subscription config to have the engine sign each payload as `x-fluxbase-signature: sha256={hex}`.

---

### Workflow Engine

`workflow/engine.rs`

Event-driven multi-step workflow execution.

**Trigger phase:**
- When an event fires, `WorkflowEngine::trigger` checks for workflows whose `trigger_event` matches `event_type`, `table.*`, or `*`.
- Creates a `workflow_executions` record with `status = running` and initial context.

**Advancement phase (background worker, 2 s tick):**
- Claims `running` executions via `FOR UPDATE SKIP LOCKED`.
- Loads the next workflow step.
- Dispatches the step action via the events dispatcher (same `webhook` / `function` / `queue_job` targets).
- Advances `current_step` on success or marks `failed` on error.
- Marks `completed` when no further steps exist.

---

### Cron Worker

`cron/worker.rs`

Fires scheduled cron jobs on a 30-second polling interval.

- Selects jobs where `enabled = TRUE AND next_run_at <= now()` using `FOR UPDATE SKIP LOCKED` (parallel-replica safe).
- Dispatches up to 50 jobs per tick via the shared dispatcher.
- Computes and stores `next_run_at` regardless of dispatch success.
- Schedule format: 5-field cron expression (e.g. `0 9 * * 1` = every Monday at 09:00 UTC). Internally prepended with a `0` for the 6-field `cron` crate.

---

## Database Schema

All metadata lives in the **`fluxbase_internal`** Postgres schema, never exposed to end users.

| Table | Purpose |
|---|---|
| `policies` | Role-based access control rules per (tenant, project, table, role, operation) |
| `table_hooks` | Lifecycle hook registrations linking table events to function IDs |
| `column_metadata` | Extended column type info: `fb_type` (`default` / `file` / `computed` / `relation`), computed expressions, file visibility |
| `relationships` | Foreign-key relationship definitions used by the relational compiler for nested queries |
| `events` | Emitted event records with delivery status and retry tracking |
| `event_subscriptions` | Per-event subscription targets (webhook / function / queue_job) |
| `event_deliveries` | Delivery attempt history with status and backoff metadata |
| `workflows` | Workflow definitions with trigger event patterns |
| `workflow_steps` | Ordered steps for each workflow (`action_type`, `action_config`) |
| `workflow_executions` | Runtime execution state (current step, context, status) |
| `cron_jobs` | Cron job definitions with schedule, action, and `next_run_at` |

| `state_mutations` | Append-only log of every INSERT/UPDATE/DELETE with before/after snapshots, versioned per row, linked to `request_id` and `span_id` — **live; powers `flux why`, `flux state history`, `flux state blame`, `flux trace diff`, `flux trace debug`, and `flux incident replay`** |

User tables live in **project-scoped schemas** named `t_{tenant_slug}_{project_slug}_{db_name}`.

---

## Deterministic Execution & Replay

This section documents how Fluxbase supports `flux why`, `flux trace replay`, `flux incident replay`, `flux state blame`, `flux trace diff`, `flux trace debug`, and `flux bug bisect`. The `state_mutations` table is live and records every INSERT/UPDATE/DELETE within the same transaction as the user-facing operation. The `span_id` column (migration `20260309000011_span_id`) links each mutation to the runtime span that caused it, enabling intra-request time-travel.

---

### Execution Trace Chain

A fully instrumented execution produces a linked chain of records:

```
trace_requests.request_id          (gateway / API)
       │
       ├─▶ runtime spans           (function execution, tool calls)
       │
       ├─▶ state_mutations         (data engine INSERT/UPDATE/DELETE)
       │       ├─ before JSONB
       │       ├─ after  JSONB
       │       └─ version BIGINT   (per-row monotonic counter)
       │
       └─▶ event_deliveries        (async fan-out)
```

Every layer references `request_id`, forming a single traceable unit across all services.

---

### `state_mutations` Table

Every INSERT, UPDATE, and DELETE executed by the data engine is written to an append-only mutations log **within the same transaction** as the user-facing operation. This is the foundation for all time-travel, replay, and debugging features.

**This table is live as of migration `20260309000007_add_state_mutations`.** The before/after JSONB columns are what power field-level diffs: the CLI compares `before_state` and `after_state` key-by-key to produce the per-field `old → new` display in `flux why`, `flux trace diff`, and `flux state history`.

`state_mutations` powers:
- `flux why` — all mutations for a request, with field-level diffs
- `flux state history` — version history for a single row
- `flux state blame` — last writer per row across a table
- `flux trace diff` — mutation comparison between two executions
- **`flux trace debug` — step-through a production request; reconstructs backend state at every span using `span_id`**
- `flux incident replay` — fetch mutations for a request or time window, then re-apply them

```sql
CREATE TABLE fluxbase_internal.state_mutations (
    mutation_id   UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id    UUID,                    -- links to trace_requests.request_id
    span_id       TEXT,                    -- links to the runtime span that triggered this (added: 20260309000011_span_id)

    tenant_id     UUID        NOT NULL,
    project_id    UUID        NOT NULL,
    schema_name   TEXT        NOT NULL,
    table_name    TEXT        NOT NULL,
    record_pk     JSONB       NOT NULL,    -- primary key value(s) of the affected row

    operation     TEXT        NOT NULL,    -- 'insert' | 'update' | 'delete'

    before        JSONB,                   -- NULL for inserts
    after         JSONB,                   -- NULL for deletes

    version       BIGINT      NOT NULL,    -- monotonically increasing per (tenant, project, table, record_pk)
    schema_version TEXT,                   -- git SHA or migration version of the schema at mutation time

    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_state_mutations_row
    ON fluxbase_internal.state_mutations(tenant_id, project_id, table_name, record_pk);

CREATE INDEX idx_state_mutations_request
    ON fluxbase_internal.state_mutations(request_id)
    WHERE request_id IS NOT NULL;

-- Time-range queries: flux incident replay 15:00..15:05
CREATE INDEX idx_state_mutations_time
    ON fluxbase_internal.state_mutations(tenant_id, project_id, table_name, created_at);

-- Row history queries: flux state blame users 42
-- SELECT ... WHERE table_name='users' AND record_pk='{"id":42}' ORDER BY version DESC LIMIT 20
CREATE INDEX idx_state_mutations_pk_version
    ON fluxbase_internal.state_mutations(tenant_id, project_id, table_name, record_pk, version DESC);
```

**Why `version` matters:**

```
users.id = 42
  version 1  →  INSERT  (after:  {name: "Alice", plan: "free"})
  version 2  →  UPDATE  (before: {plan: "free"}, after: {plan: "pro"})
  version 3  →  UPDATE  (before: {plan: "pro"},  after: {plan: "free"})
```

This enables `flux state blame users 42` to pinpoint exactly which request caused each change.

**Why `request_id` matters:**

With `request_id` linked to `trace_requests`, every mutation becomes answerable:

```
Row users.id=42 modified by request 550e8400
  endpoint:  POST /signup
  function:  create_user
  commit:    a93f42c
  triggered: workflow step 3 of onboarding_flow
```

---

### Replay Mode

For replay to be safe (no side effects), the data engine must support a **replay execution mode** that disables all outbound activity.

A caller activates replay mode by setting:

```
x-flux-replay: true
```

When this header is present, the following subsystems are **bypassed**:

| Subsystem | Normal mode | Replay mode |
|---|---|---|
| Before hooks | Invoked, can abort | Skipped |
| After hooks | Invoked asynchronously | Skipped |
| Event emitter | Writes to `events` table | Skipped |
| Workflow triggers | Creates `workflow_executions` | Skipped |
| Cron advancement | Fires due jobs | Skipped |
| External dispatch | Webhook / function / queue | Skipped |

Only the read/write to the user tables and the append to `state_mutations` are executed. This ensures that replaying an historical sequence of requests produces the same data state without re-triggering notifications or workflows.

---

### DB Query Trace Correlation

Postgres queries are not automatically correlated to the request that issued them. To connect gateway logs → runtime spans → Postgres query logs, prepend a comment or use `SET LOCAL`:

**Option A — SQL comment (visible in `pg_stat_activity`, `auto_explain`):**
```sql
/* flux_request_id:550e8400-..., span_id:abc123 */
SELECT ...
```

**Option B — `application_name` per transaction:**
```sql
SET LOCAL application_name = 'flux:550e8400';
SELECT ...
```

Option A is preferred because it survives connection pooling and appears in `pgaudit` logs without session-level side effects.

---

## Configuration Reference

All configuration is from environment variables (loaded via `dotenvy`).

| Variable | Default | Description |
|---|---|---|
| `DATABASE_URL` | — *required* | Postgres connection string |
| `PORT` / `DATA_ENGINE_PORT` | `8080` | HTTP listen port |
| `DEFAULT_QUERY_LIMIT` | `100` | Rows returned when LIMIT is omitted |
| `MAX_QUERY_LIMIT` | `5000` | Hard ceiling on LIMIT |
| `RUNTIME_URL` | `http://localhost:8082` | Base URL of the runtime service for hook/function dispatch |
| `INTERNAL_SERVICE_TOKEN` | `fluxbase_secret_token` | Shared token required on every request |
| `MAX_QUERY_COMPLEXITY` | `1000` | Complexity score ceiling; `0` disables the check |
| `QUERY_TIMEOUT_MS` | `30000` | Execution timeout in milliseconds |
| `MAX_NEST_DEPTH` | `6` | Maximum relationship nesting depth; `0` disables |
| `FILES_BUCKET` | — | S3 bucket for file uploads; omit to disable |
| `S3_BUCKET` | — | Legacy alias for `FILES_BUCKET` |
| `S3_REGION` | `us-east-1` | AWS region |
| `S3_ENDPOINT` | — | Custom S3 endpoint (MinIO, Localstack) |
| `RUST_LOG` | `data_engine=debug` | Tracing filter |

---

## Deployment Notes

- **Migrations must be applied before deploy.** Run `make migrate SERVICE=data-engine` explicitly; the service does not run migrations on startup (avoids Neon cold-start hang from `pg_advisory_lock`).
- **Ingress:** Can run with `--ingress all` safely because `INTERNAL_SERVICE_TOKEN` gates all non-health endpoints.
- **Multiple replicas:** Safe — all workers use `FOR UPDATE SKIP LOCKED` to avoid duplicate dispatch.
- **Cache warm-up:** The schema and plan caches start empty and warm up on first access per table. There is no pre-warming mechanism; the first request per table pays the DB round-trip cost.

---

## Architectural Gaps & Improvements

Ordered by impact. Items marked **[blocking]** must be closed before production replay/debugging features can work. Items marked **[hardening]** improve safety and correctness. Items marked **[performance]** are optimisations.

---

### Gap 1 — State Mutation Logging `[resolved]`

The `fluxbase_internal.state_mutations` table is live. Every INSERT/UPDATE/DELETE is captured within the same Postgres transaction as the user mutation in `db_executor.rs`. `before_state` and `after_state` JSONB columns are populated using `RETURNING` pre-images. `request_id` is populated from the `x-request-id` header. `version` is incremented per `(tenant, project, table, record_pk)` atomically.

`flux why`, `flux state history`, `flux state blame`, `flux trace diff`, and `flux incident replay` all function against live production data.

---

### Gap 2 — Hook Trace Propagation `[blocking]`

Hook invocations (`POST {RUNTIME_URL}/internal/execute`) do not forward the caller's trace headers. The trace chain breaks here:

```
gateway → runtime → data engine mutation → hook runtime
```

**Required action:** Inject into every hook/event/workflow dispatch call:
- `x-request-id` — from the originating request
- `x-parent-span-id` — current span at the point of dispatch
- `x-code-sha` — git commit SHA of the deployed function

Without these, hook execution cannot be attributed to the request that triggered it.

---

### Gap 3 — DB Query Trace Correlation `[blocking]`

Postgres queries are not correlated to the request that issued them, making it impossible to match gateway/runtime logs to specific database operations in `pg_stat_activity`, `auto_explain`, or `pgaudit`.

**Required action:** Prepend a SQL comment to every compiled query before execution:

```sql
/* flux_request_id:{{uuid}}, span_id:{{uuid}} */
SELECT ...
```

This is zero-overhead (comment is stripped by the parser) and survives connection pooling unlike `SET LOCAL application_name`.

---

### Gap 4 — Replay Mode (`x-flux-replay`) `[blocking]`

Replaying historical mutations would re-trigger hooks, events, workflow executions, and cron jobs, producing real side effects (emails sent, webhooks fired, etc.).

**Required action:** Add an `AuthContext` flag `is_replay: bool` populated from the `x-flux-replay: true` header. In the query handler, skip the following subsystems when `is_replay` is true:
- Before / after hook invocations
- `EventEmitter::emit`
- `WorkflowEngine::trigger`
- Any cron advancement

State mutations should still be written (allows replay to produce a reconstructed data state). See the [Replay Mode](#replay-mode) section.

---

### Gap 5 — Transaction-scoped `search_path` Enforcement `[hardening]`

Every query in `db_executor.rs` uses fully-qualified table names generated by the compiler (`schema.table`), but **nothing at the database session level prevents an unqualified name from resolving to the wrong schema**. If a future code path, raw query, or hook omits the schema prefix, Postgres will fall back to the session `search_path`, which defaults to `"$user", public`. In a multi-tenant environment this is a cross-tenant data leak waiting to happen.

**Required action:** At the start of every transaction in `db_executor.rs`, execute:

```sql
SET LOCAL search_path = "t_{tenant}_{project}_{db}", public;
```

`SET LOCAL` scopes the change to the current transaction only — it does not affect other queries on the same pooled connection and requires no teardown. The cost is approximately 0.02 ms per transaction.

**Effect:**

| Scenario | Without `SET LOCAL` | With `SET LOCAL` |
|---|---|---|
| Missing schema prefix in compiled SQL | Resolves via session `search_path` — potentially wrong tenant | Resolves to correct tenant schema |
| SQL injection attempt via schema bypass | May succeed if `search_path` is predictable | Contained to tenant schema |
| Developer forgets prefix in a future raw query | Silent cross-tenant read | Query fails with `relation not found` (detectable) |

**Implementation location:** `executor/db_executor.rs`, immediately after `pool.begin()`, before any user SQL executes.

---

### Gap 6 — Multi-Tenant Identifier Isolation `[hardening]`

`validate_identifier` guards against schema-name injection, but the allowed character set should be explicitly tested and documented to prevent edge cases.

**Required action:**
- Enforce `[a-z0-9_]+` (regex) after lowercasing. Reject anything else with a clear error.
- Add a unit test suite covering: SQL keywords, Unicode characters, control characters, hyphen (should be converted, not rejected), double-quote, semicolon, and NULL bytes.
- Ensure the regex is applied to every component: `tenant_slug`, `project_slug`, and `db_name` independently before concatenation.

---

### Gap 7 — Policy Cache Uses `O(n)` Eviction `[performance]`

The policy cache is a `RwLock<HashMap>` that evicts by iterating all keys and filtering on prefix match. Under high write concurrency (frequent policy changes), this can stall all policy reads.

**Recommended action:** Migrate to a Moka cache (same library as schema and plan caches), keyed by the existing `"tenant:project:table:role:op"` string. Benefits: automatic TTL, LRU eviction, no manual eviction code, and consistent with the other two cache layers.

---

### Gap 8 — Cron Failure Tracking `[hardening]`

`next_run_at` is always advanced regardless of whether the dispatch succeeded. A job that consistently fails silently keeps firing.

**Recommended action:**
- Add `failure_count INT NOT NULL DEFAULT 0` and `last_error TEXT` to `cron_jobs`.
- On dispatch failure, increment `failure_count` and optionally disable the job after N consecutive failures.
- Expose failure state in `GET /db/cron` response.

---

### Gap 9 — Event Dead-Letter Queue `[hardening]`

Events that exhaust retry attempts remain in a non-terminal state. There is no way to inspect or re-process them.

**Recommended action:**
- Add a `dead_letter` terminal status to `event_deliveries`.
- After exhausting the retry budget, mark the delivery `dead_letter` and log a structured error.
- Expose `GET /db/dead-letters` for operator inspection and manual re-queue.

---

### Gap 10 — Workflow Step Timeout `[hardening]`

A workflow step that does not return from the runtime leaves the execution perpetually in `running` state. The 2 s worker tick will keep detecting it and attempting re-advance, potentially causing duplicate dispatches.

**Recommended action:**
- Add `stepped_at TIMESTAMPTZ` to `workflow_executions`.
- In the advancement worker, skip (or mark `timed_out`) executions where `stepped_at < now() - interval '5 minutes'`.
- Use the existing `FOR UPDATE SKIP LOCKED` pattern to ensure only one replica touches a given execution per tick.

---

### Gap 11 — Plan Cache Covers SELECT Only `[performance]`

INSERT/UPDATE/DELETE are fully recompiled on every request. For write-heavy workloads (e.g. bulk ingestion), this adds meaningful CPU cost.

**Recommended action:** Extend the plan cache to store compiled mutation SQL templates. The cache key should include the operation, column list shape, and policy fingerprint. Bind parameters are still rebuild per-request.

---

### Other Items

| Area | Item |
|---|---|
| Security | `INTERNAL_SERVICE_TOKEN` should not have a default value in production — require it or fail at startup |
| Security | Consider HMAC request signing between API ↔ Data Engine for defence-in-depth |
| Observability | Complexity scores and cache hit/miss rates logged at `debug` only; surface as Prometheus counters |
| Observability | `/db/debug` endpoint contents are undocumented — review exposure and whether it needs elevated auth |
| Features | Computed columns are injected as raw SQL without sandboxing — consider an expression whitelist |
| Features | File columns: no image resizing, CDN invalidation, or lifecycle policies |
| Features | Relationships: no automatic many-to-many pivot detection |
| Features | No built-in soft-delete convention (`deleted_at`) or operator audit log |
| Reliability | Workflow LISTEN/NOTIFY: replace 2 s unconditional poll with Postgres pub/sub to reduce idle DB load |
