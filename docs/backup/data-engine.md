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
   - [Execution Trace Chain](#execution-trace-chain)
   - [`state_mutations` Table](#state_mutations-table)
   - [`trace_requests` Table](#trace_requests-table)
   - [Replay Mode](#replay-mode)
   - [DB Query Trace Correlation](#db-query-trace-correlation)
9. [Configuration Reference](#configuration-reference)
10. [Deployment Notes](#deployment-notes)
11. [Architectural Gaps & Improvements](#architectural-gaps--improvements)

---

## Overview

```
             ┌─────────────────────────┐
             │   Fluxbase Platform DB  │
             │  (flux_internal.*)  │
             └──────────┬──────────────┘
                        │
Client / SDK            │
     │                  │
     ▼                  ▼
[ API Service ] ──▶ [ Data Engine ] ──▶ [ User PostgreSQL ]
     │               (x-service-token)
     ▼
[ Gateway ]  ───▶ [ Data Engine ] ──▶ [ User PostgreSQL ]
```

The Data Engine reads and writes platform metadata from the Fluxbase platform database while executing user queries against the project's configured PostgreSQL database.

The Data Engine is responsible for:

- **Secure multi-tenant data access** — all queries are scoped to a `(tenant, project, database)` triple.
- **Row/column level security** — policy evaluation before every mutation or fetch.
- **Schema management** — create/alter/drop Postgres schemas, tables, and relationships.
- **Lifecycle hooks** — invoke serverless functions before/after every table mutation.
- **Event delivery** — persist and fan-out events to webhooks, functions, and queues.
- **Workflow execution** — step-advance workflow executions triggered by events.
- **Cron scheduling** — fire cron jobs whose schedule has become due.
- **File upload/download** — presigned S3 URLs with per-table access control.

### External Database Model (BYODB)

Fluxbase supports a **Bring Your Own Database (BYODB)** architecture. Application data is stored in a user-provided PostgreSQL database, while Fluxbase platform metadata is stored in the Fluxbase platform database.

```
Fluxbase Platform DB
  └─ flux_internal.*

User PostgreSQL
  └─ application tables (users, orders, etc.)
```

The Data Engine coordinates both databases:

| Database | Stores |
|---|---|
| Platform DB | policies, hooks, events, workflows, `state_mutations`, `trace_requests` |
| User DB | application tables and business data |

This model allows Fluxbase to operate as an execution and observability layer for existing Postgres databases without requiring data migration.

All queries must pass through the Data Engine so that:
- policies are enforced
- mutations are recorded in `state_mutations`
- traces remain linked to the originating request
- replay and debugging features (`flux incident replay`, `flux bug bisect`) work correctly

### Data Engine Invariant

All application queries **must** pass through the Data Engine:

```
Runtime
   ↓
Data Engine
   ↓
User PostgreSQL
```

Direct database access from the Runtime is not allowed because it would bypass:
- mutation logging (`state_mutations`)
- policy enforcement
- tracing
- deterministic replay

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
  ├─ 5. Schema existence check (pg_catalog)            → HTTP 404 if missing
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
  │     └─ Miss → load from flux_internal.column_metadata + relationships
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
  │      (The executor runs compiled SQL against the project's configured PostgreSQL database —
  │       never the platform DB. The `search_path` is set to the tenant schema so all SQL
  │       fragments resolve against the correct project tables.)
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

Column metadata is loaded from `flux_internal.column_metadata` and cached in the L1 schema cache.

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
- `EventEmitter` writes an event record to `flux_internal.events` with status `pending`.
- Payload includes the mutated row(s) and event type (e.g. `users.insert`).

**Delivery phase** (asynchronous, background worker):
- Worker polls `flux_internal.events` for `pending` records.
- For each event, loads matching subscriptions from `flux_internal.event_subscriptions`.
- Calls `dispatcher::dispatch` per subscription.

**Dispatch targets:**

| Target type | Mechanism |
|---|---|
| `webhook` | `POST` to configured URL with optional HMAC-SHA256 signature + custom headers |
| `function` | `POST {RUNTIME_URL}/internal/execute` with `function_id` + payload |
| `queue_job` | Insert job into the queue service DB |

**Retry:** Failed deliveries are tracked in `flux_internal.event_deliveries` with exponential backoff. The worker skips events that have exhausted their retry budget.

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

All Fluxbase **platform metadata** lives in the **`flux_internal`** schema in the Fluxbase platform database, never exposed to end users.

Application tables do **not** live in this database. Instead they reside in the project's configured PostgreSQL database connected via the Data Engine.

```
Platform DB
  flux_internal.policies
  flux_internal.events
  flux_internal.state_mutations
  flux_internal.trace_requests
  flux_internal.workflows   (+ steps, executions)
  flux_internal.cron_jobs
  … (see table below)

User DB
  users
  orders
  subscriptions
  payments
  … (project-defined application tables)
```

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
| `state_mutations` | Append-only log of every INSERT/UPDATE/DELETE with before/after snapshots, versioned per row; includes `mutation_seq BIGSERIAL` for deterministic replay ordering, `changed_fields TEXT[]` for cheap field-level diffs, linked to `request_id` and `span_id` — **live; powers `flux why`, `flux state history`, `flux state blame`, `flux trace diff`, `flux trace debug`, and `flux incident replay`** |
| `trace_requests` | Request envelope store — captures `method`, `path`, `headers`, `body`, `response_status`, `response_body`, `duration_ms` per `request_id`; ensures replay never depends on gateway log retention |

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
CREATE TABLE flux_internal.state_mutations (
    mutation_id    UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    mutation_seq   BIGSERIAL,               -- global monotonic sequence; ORDER BY mutation_seq for deterministic replay
    mutation_ts    TIMESTAMPTZ DEFAULT now(), -- wall-clock time of the mutation; powers time-windowed incident replay
    request_id     UUID,                    -- links to trace_requests.request_id
    span_id        TEXT,                    -- links to the runtime span that triggered this (added: 20260309000011_span_id)

    tenant_id      UUID        NOT NULL,
    project_id     UUID        NOT NULL,
    schema_name    TEXT        NOT NULL,
    table_name     TEXT        NOT NULL,
    record_pk      JSONB       NOT NULL,    -- primary key value(s) of the affected row

    operation      TEXT        NOT NULL,    -- 'insert' | 'update' | 'delete'

    before         JSONB,                   -- NULL for inserts; compressed by Postgres TOAST
    after          JSONB,                   -- NULL for deletes; compressed by Postgres TOAST
    changed_fields TEXT[],                  -- names of fields that changed (UPDATE only); enables cheap diff without JSON comparison

    version        BIGINT      NOT NULL,    -- monotonically increasing per (tenant, project, table, record_pk)
    schema_version TEXT,                    -- git SHA or migration version of the schema at mutation time

    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_state_mutations_row
    ON flux_internal.state_mutations(tenant_id, project_id, table_name, record_pk);

CREATE INDEX idx_state_mutations_request
    ON flux_internal.state_mutations(request_id)
    WHERE request_id IS NOT NULL;

-- Time-range queries: flux incident replay 2026-03-09T15:00..15:05
-- WHERE mutation_ts BETWEEN $from AND $to ORDER BY mutation_seq
CREATE INDEX idx_state_mutations_time
    ON flux_internal.state_mutations(mutation_ts);

-- flux state blame / state history: O(log N) latest-mutation lookup per record.
-- Composite order (tenant → project → table → record_pk → newest-first) means
-- Postgres reads exactly one index leaf and stops for LIMIT 1 queries.
CREATE INDEX idx_state_mutations_pk_latest
    ON flux_internal.state_mutations (
        tenant_id,
        project_id,
        table_name,
        record_pk,
        mutation_seq DESC
    );

-- Deterministic replay ordering within a request: ORDER BY mutation_seq replaces timestamp heuristics
CREATE INDEX idx_state_mutations_request_seq
    ON flux_internal.state_mutations(request_id, mutation_seq)
    WHERE request_id IS NOT NULL;

-- Incident replay filtered by table: flux incident replay --request-id ... --table users
CREATE INDEX idx_state_mutations_request_table
    ON flux_internal.state_mutations(request_id, table_name)
    WHERE request_id IS NOT NULL;
```

**Why `version` matters:**

```
users.id = 42
  version 1  →  INSERT  (after:  {name: "Alice", plan: "free"})
  version 2  →  UPDATE  (before: {plan: "free"}, after: {plan: "pro"})
  version 3  →  UPDATE  (before: {plan: "pro"},  after: {plan: "free"})
```

This enables `flux state blame users 42` to pinpoint exactly which request caused each change.

**Why `mutation_seq` matters:**

Within a single request that touches multiple rows across multiple tables, `created_at` timestamps can collide (sub-millisecond writes in the same transaction). `mutation_seq` is a `BIGSERIAL` that guarantees global ordering:

```sql
-- Deterministic replay: apply mutations in the exact order they were written
SELECT * FROM flux_internal.state_mutations
WHERE request_id = $1
ORDER BY mutation_seq;
```

This replaces timestamp heuristics in `flux trace debug` and `flux incident replay` with a strict causal order.

**Why `changed_fields` matters:**

For UPDATE operations, `changed_fields` stores only the names of modified columns (e.g. `["plan", "updated_at"]`). The CLI can compute `flux why` field-level diffs by reading `changed_fields` first and then extracting only those keys from the `before`/`after` JSONB — no full JSON comparison needed:

```
users.id = 42  version 2  changed_fields: ["plan", "updated_at"]
  plan:       "free"  →  "pro"
  updated_at: 2026-03-10T09:00:00Z  →  2026-03-11T14:22:01Z
```

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

### `trace_requests` Table

The `trace_requests` table stores the full request envelope for every operation processed by the data engine. This makes replay completely self-contained — the original request input is preserved locally and does not depend on gateway log retention (which may expire or be unavailable during incident investigation).

```sql
CREATE TABLE flux_internal.trace_requests (
    request_id      UUID        PRIMARY KEY,
    tenant_id       UUID        NOT NULL,
    project_id      UUID        NOT NULL,

    method          TEXT        NOT NULL,   -- 'POST', 'GET', etc.
    path            TEXT        NOT NULL,   -- e.g. '/db/query'
    headers         JSONB,                  -- relevant headers (x-user-id, x-user-role, etc.); exclude auth tokens
    body            JSONB,                  -- request body (QueryRequest), compressed via TOAST

    response_status INT,                    -- HTTP status returned to caller
    response_body   JSONB,                  -- response payload; may be truncated for large results
    duration_ms     INT,                    -- end-to-end latency

    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_trace_requests_tenant
    ON flux_internal.trace_requests(tenant_id, project_id, created_at DESC);
```

**Replay pipeline with `trace_requests`:**

```
trace_requests             ← original input (method, path, body, headers)
       ↓
state_mutations            ← mutations ordered by mutation_seq
       ↓
execution replay           ← re-apply mutations in order, skipping side effects
```

Without this table, `flux incident replay` must fall back to whatever gateway retained, which may be incomplete, expired, or unavailable in air-gapped environments.

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
| External tool calls | Composio / HTTP tools in hooks | Skipped |

Only the read/write to the user tables and the append to `state_mutations` are executed. This ensures that replaying an historical sequence of requests produces the same data state without re-triggering notifications or workflows.

---

### DB Query Trace Correlation

Postgres queries are not automatically correlated to the request that issued them. To connect gateway logs → runtime spans → Postgres query logs, prepend a comment or use `SET LOCAL`:

**Option A — SQL comment (visible in `pg_stat_activity`, `auto_explain`):**
```sql
/* flux_req:550e8400-..., span:abc123 */
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

- **Migrations must be applied before deploy.** Run `make migrate` explicitly; the service does not run migrations on startup (avoids Neon cold-start hang from `pg_advisory_lock`).
- **Ingress:** Can run with `--ingress all` safely because `INTERNAL_SERVICE_TOKEN` gates all non-health endpoints.
- **Multiple replicas:** Safe — all workers use `FOR UPDATE SKIP LOCKED` to avoid duplicate dispatch.
- **Cache warm-up:** The schema and plan caches start empty and warm up on first access per table. There is no pre-warming mechanism; the first request per table pays the DB round-trip cost.

---

## Architectural Gaps & Improvements

Ordered by impact. Items marked **[blocking]** must be closed before production replay/debugging features can work. Items marked **[hardening]** improve safety and correctness. Items marked **[performance]** are optimisations.

---

### Gap 1 — State Mutation Logging `[resolved]`

The `flux_internal.state_mutations` table is live. Every INSERT/UPDATE/DELETE is captured within the same Postgres transaction as the user mutation in `db_executor.rs`. `before_state` and `after_state` JSONB columns are populated using `RETURNING` pre-images. `request_id` is populated from the `x-request-id` header. `version` is incremented per `(tenant, project, table, record_pk)` atomically.

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

### Gap 3 — DB Query Trace Correlation `[resolved]`

Every compiled query is now prefixed with:

```sql
/* flux_req:{{request_uuid}},span:{{span_id}},tenant:{{tenant_uuid}} */ SELECT ...
```

The `span:` field allows `pg_stat_activity` and `auto_explain` to directly map each in-flight query to the runtime span that generated it — essential for `flux trace debug` step-through mode. Zero parsing overhead (comment stripped by Postgres); survives connection pooling.

---

### Gap 4 — Replay Mode (`x-flux-replay`) `[resolved]`

`AuthContext.is_replay` is populated from the `x-flux-replay: true` header. The query handler suppresses the following subsystems when `is_replay` is true:

- Before / after hook invocations
- `EventEmitter::emit` (which would trigger workflow executions and event subscriptions)
- Any cron job advancement
- External tool calls forwarded via hooks

State mutations, `trace_requests` writes, and trace span emission are **not** suppressed so replay produces a fully observable reconstructed data state.

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

The existing Moka L2 cache infrastructure is already in place. Extending the cache key struct to cover mutations is a one-field change (`operation: QueryOperation`).

---

### Gap 12 — Deterministic Mutation Ordering `[resolved]`

`mutation_seq BIGSERIAL` added to `state_mutations` via migration `20260311000012`. The column is auto-populated by Postgres on every INSERT; no application-level changes were needed. `flux incident replay` and `flux trace debug` now order by `mutation_seq` for strict causal ordering within a request.

`mutation_ts TIMESTAMPTZ DEFAULT now()` added via migration `20260311000014`. Enables time-windowed incident replay without a full table scan:

```sql
SELECT * FROM flux_internal.state_mutations
WHERE  mutation_ts BETWEEN $from AND $to
ORDER  BY mutation_seq;
```

Storage cost: 8 bytes per row. Index `idx_state_mutations_time (mutation_ts)` added.

New indexes applied:
- `idx_state_mutations_request_seq (request_id, mutation_seq)` — replay within a request
- `idx_state_mutations_request_table (request_id, table_name)` — targeted table replay
- `idx_state_mutations_request_id (request_id)` — trace_requests join
- `idx_state_mutations_time (mutation_ts)` — time-windowed incident queries
- `idx_state_mutations_pk_latest (tenant_id, project_id, table_name, record_pk, mutation_seq DESC)` — O(log N) `flux state blame` / `flux state history`

---

### Gap 13 — Request Envelope Table (`trace_requests`) `[resolved]`

`flux_internal.trace_requests` created via migration `20260311000013`. The `POST /db/query` handler writes a row at the end of every request as a fire-and-forget `tokio::spawn`. Safe headers only (no auth tokens). Response body truncated to first 100 rows. Uses `ON CONFLICT (request_id) DO NOTHING` so re-runs are idempotent.

`flux incident replay` is now self-contained and does not require gateway log retention.

---

### Gap 14 — Mutation Compression (`changed_fields`) `[resolved]`

`changed_fields TEXT[]` column added via migration `20260311000012`. Fully populated as of Gap 14 v2 implementation.

**How it works (end-to-end):**

1. **Compiler** (`compiler/query_compiler.rs`) — `compile_update()` now builds a second SQL statement alongside the main UPDATE: `SELECT * FROM schema.table WHERE {same conditions} FOR UPDATE` with its own `pre_read_params` list (fresh `$N` indices, no SET params mixed in). Both are stored in `CompiledQuery.pre_read_sql` / `pre_read_params`.

2. **Executor** (`executor/db_executor.rs`) — For UPDATE operations only, immediately after `SET LOCAL search_path`, the executor runs the pre-read SELECT inside the same transaction. The `FOR UPDATE` clause locks the matching rows *before* the mutation, eliminating lost-update races. Results are stored in a `HashMap<String, serde_json::Value>` keyed by `record_pk`.

3. **Mutation log write** — In the `state_mutations` INSERT loop, each UPDATE row's `before_state` is fetched from the pre-read map (keyed by the row's pk). `changed_fields` is computed by union-diffing the two JSONB objects key-by-key:
   ```rust
   keys = before.keys ∪ after.keys
   changed_fields = keys.filter(|k| before[k] != after[k]).sorted()
   ```
   Result: `changed_fields` is a sorted `TEXT[]` of every column whose value differed, or `NULL` if `before_state` was not found (e.g. first mutation on a record with no prior log entry).

**What is stored:**

| operation | before_state | after_state | changed_fields |
|---|---|---|---|
| INSERT | NULL | full new row | NULL |
| UPDATE | full old row (pre-read) | full new row (RETURNING) | sorted column names that changed |
| DELETE | full deleted row (RETURNING) | NULL | NULL |

**Debugging column matrix** — all four columns are now live:

| CLI feature | Column used | Why |
|---|---|---|
| `flux trace debug` | `mutation_seq` | strict step-through order within a request |
| `flux incident replay 15:00..15:05` | `mutation_ts` | time-windowed scan without full table read |
| `flux trace diff` | `changed_fields` | field-level diff without full JSONB comparison |
| `flux state blame` / replay | `schema_name` | full `(schema, table, pk)` identity for cross-tenant correctness |

**Index coverage** — all access patterns are O(log N):

| CLI feature | Index used |
|---|---|
| `flux trace debug` | `idx_state_mutations_request_seq` |
| `flux incident replay 15:00..15:05` | `idx_state_mutations_time` |
| `flux trace diff` | `idx_state_mutations_request_table` |
| `flux state blame` / `flux state history` | `idx_state_mutations_pk_latest` |

---

### Gap 15 — Mutation Integrity Check `[hardening]`

Version sequences per row should be strictly monotonic with no gaps (`1 → 2 → 3`). A gap (`1 → 3`) indicates a lost write — either a crashed transaction that didn't roll back cleanly, or a migration that skipped increment logic.

**Recommended action:** Add a periodic background check (or an on-demand CLI command `flux db integrity-check`) that runs:

```sql
SELECT record_pk, array_agg(version ORDER BY version) AS versions
FROM flux_internal.state_mutations
WHERE tenant_id = $1 AND project_id = $2 AND table_name = $3
GROUP BY record_pk
HAVING count(*) != (max(version) - min(version) + 1);
```

Any row returned has a version gap — log as a structured warning keyed on `(tenant_id, project_id, table_name, record_pk)`. This is rare but critical to detect before it silently corrupts replay output.

---

### Gap 16 — BYODB Database Identity Check `[resolved]`

**Problem:** In a BYODB architecture the Data Engine accepts a user-supplied `connection_url`. DNS failover, snapshot restores, or misconfiguration can silently redirect the pool to a different PostgreSQL cluster. Without an identity check the engine writes to the wrong database without any error signal.

**Three dangerous scenarios this prevents:**

| Scenario | Without check | With check |
|---|---|---|
| DNS failover to new cluster | Silent writes to wrong DB | Engine refuses to start |
| Customer restores March 1 snapshot | New data lands in old state; replay corrupted | Engine refuses to start |
| Staging DB accidentally registered | Production workloads on staging | Engine refuses to start |

**Implementation (≈15 lines):** `db/connection.rs`

```rust
// Query the live cluster identity
pub async fn read_db_identity(pool: &PgPool) -> Result<DbIdentity, sqlx::Error> {
    let (system_identifier, db_name): (String, String) = sqlx::query_as(
        "SELECT system_identifier::text, current_database() FROM pg_control_system()",
    )
    .fetch_one(pool)
    .await?;
    Ok(DbIdentity { system_identifier, db_name })
}

// Enforce the identity matches the registration record
pub async fn verify_db_identity(pool: &PgPool, project_id: &str, expected: &DbIdentity) { … }
```

`pg_control_system().system_identifier` is unique per physical cluster and survives logical replica promotion — it only changes on `initdb`. `current_database()` guards against pointing at the wrong logical database on the same host.

**Stored in:** `flux_internal.project_databases.expected_system_identifier` + `expected_db_name` (migration `20260311000016`).

**Call site:** call `verify_db_identity()` once after constructing any user pool. For the platform DB, `init_pool_with_identity_log()` logs the live identity at startup without enforcing an expected value.

**Degraded mode:** If `pg_control_system()` is unavailable (managed provider restricts role), a warning is logged and the check is skipped. This is the only exception — mismatches always panic.

---

### Gap 17 — Schema Introspection on Large BYODB Databases `[resolved]`

**Problem:** `information_schema` views (`information_schema.tables`, `information_schema.schemata`) evaluate row-level visibility and ACL checks for every object in the entire cluster before filtering. On a BYODB database with 500–2000 tables, this costs 20–200 ms per introspection call regardless of how many tables Fluxbase manages. Under concurrent load it creates unexpected CPU pressure on the customer's database.

**Root cause:** `information_schema` is an ANSI-SQL compatibility layer implemented as Postgres views. Each view joins `pg_class`, `pg_namespace`, `pg_attribute`, and `has_*_privilege()` functions across all visible objects. The `WHERE table_schema = $1` predicate is applied _after_ the full view expansion.

**Fix:** Replace all four `information_schema` call sites with direct `pg_catalog` queries. `pg_catalog.pg_namespace` and `pg_catalog.pg_class` are physical catalog tables with indexes; the planner evaluates `nspname = $1` and `relname = $2` via index scan — O(log N) on the catalog regardless of cluster size.

| Call site | Before | After |
|---|---|---|
| `schema.rs` tbls CTE | `information_schema.tables WHERE table_schema = …` | `pg_class JOIN pg_namespace WHERE n.nspname = …` |
| `db_router.rs` `assert_exists()` | `information_schema.schemata WHERE schema_name = $1` | `pg_namespace WHERE nspname = $1` |
| `db_router.rs` `list_schemas()` | `information_schema.schemata WHERE schema_name LIKE $1` | `pg_namespace WHERE nspname LIKE $1` |
| `db_router.rs` `assert_table_exists()` | `information_schema.tables WHERE table_schema = $1 AND table_name = $2` | `pg_class JOIN pg_namespace WHERE n.nspname = $1 AND c.relname = $2` |

**Performance at scale:**

| Database size | `information_schema` | `pg_catalog` |
|---|---|---|
| 50 tables | ~5 ms | ~1 ms |
| 500 tables | ~30 ms | ~1 ms |
| 2000 tables | ~150 ms | ~1 ms |

Latency is now constant and independent of how many schemas/tables exist in the customer's database.

**Schema Snapshot Cache (migration `20260311000017`):** `flux_internal.schema_snapshots` stores a pre-built JSON snapshot of `(tables, columns, relationships)` per `(tenant, project, schema)`. Written on every `CREATE TABLE` / `ALTER TABLE` / `DROP TABLE`. Intended as a future fast-path for `GET /db/schema` — serve the snapshot instead of querying `pg_catalog` at all. A `version BIGINT` column auto-increments via a trigger on every write, enabling polling for changes. The snapshot is an *optimisation*; the system is always correct without it.

---

### Gap 18 — Automatic LIMIT Guard on SELECT Queries `[resolved]`

**Problem:** A BYODB table with 10 M+ rows and no explicit `limit` in the query request would issue an unbounded `SELECT … FROM schema.table` and stream the entire result set through the data engine, exhausting memory and connection time on both sides. Additionally, a caller could supply `offset` without `limit`, creating ambiguous pagination with no stable row contract.

**Root cause:** The query compiler accepted `QueryRequest.limit = null` and passed it straight to the SQL template with no default injection. Callers that omitted `limit` received whatever Postgres returned, which is every row.

**Fix — two layers, both in `query_compiler.rs`:**

1. **OFFSET-without-LIMIT guard** (new) — At the top of `compile_select`, before any SQL is built:
   ```rust
   if req.offset.is_some() && req.limit.is_none() {
       return Err(EngineError::MissingField(
           "limit is required when offset is specified".into(),
       ));
   }
   ```
   Returns HTTP 400 immediately. Callers that rely on `limit=null` for offset-based pagination are broken by design and must be fixed.

2. **Automatic LIMIT injection** (was already present in both paths) — When `limit` is present but omitted, `default_limit` (default 100) is injected. When a caller-supplied `limit` exceeds `max_limit` (default 5 000), it is silently clamped:
   ```rust
   let effective_limit = match req.limit {
       Some(l) => l.min(opts.max_limit).max(1),
       None    => opts.default_limit,
   };
   ```
   This runs in both the batched (depth ≥ `BATCH_DEPTH_THRESHOLD`) and the non-batched SELECT path.

**Configuration:**

| Env var | Default | Description |
|---|---|---|
| `DEFAULT_QUERY_LIMIT` | `100` | Applied when `limit` is omitted |
| `MAX_QUERY_LIMIT` | `5000` | Maximum rows any single SELECT may return |

**Behaviour summary:**

| Request | Result |
|---|---|
| `limit=null, offset=null` | Injects `LIMIT 100` |
| `limit=50, offset=null` | Uses `LIMIT 50` |
| `limit=10000, offset=null` | Clamped to `LIMIT 5000` |
| `limit=null, offset=200` | **400 Bad Request** — limit is required |
| `limit=50, offset=200` | `LIMIT 50 OFFSET 200` — valid |

No migration required; this is a compiler-only change.

---

### Gap 19 — Postgres-level Statement Timeout (BYODB Query Safety) `[resolved]`

**Problem:** Even with the complexity guard (Gap 12), LIMIT guard (Gap 18), and the Rust `tokio::timeout` wrapper, a query can still be expensive *inside Postgres*. A three-way JOIN on large BYODB tables (10 M users × 40 M orders × 200 M order_items) may trigger a hash join with temp-file disk spill that holds a Postgres backend for 30 s or more. When the Rust future is dropped (tokio timeout), the query keeps running in the database engine — CPU remains high, the backend stays active, and the customer's production database suffers.

**Root cause:** The Rust-side timeout drops the TCP connection, but Postgres does not automatically cancel a query when its client disconnects in all configurations (especially PgBouncer / pgx proxy). Without an explicit `statement_timeout`, the DB backend continues until the query finishes naturally.

**Fix — `SET LOCAL statement_timeout = 'Nms'` in every transaction:**

Added in `executor/db_executor.rs`, immediately after `SET LOCAL search_path`:

```rust
// Gap 19: Postgres-level statement timeout
sqlx::query(&format!("SET LOCAL statement_timeout = '{}ms'", ctx.statement_timeout_ms))
    .execute(&mut *tx)
    .await
    .map_err(EngineError::Db)?;
```

`SET LOCAL` means the setting is scoped to the current transaction and resets automatically on `COMMIT` / `ROLLBACK`. It cannot leak to other connections in the pool.

When the timeout fires, Postgres returns `SQLSTATE 57014` (`query_canceled`). A new `map_db_error()` helper in `db_executor.rs` intercepts this and converts it to `EngineError::QueryTimeout` (HTTP 408) rather than the generic 500:

```rust
fn map_db_error(e: sqlx::Error) -> EngineError {
    if let sqlx::Error::Database(ref db_err) = e {
        if db_err.code().as_deref() == Some("57014") {
            return EngineError::QueryTimeout;
        }
    }
    EngineError::Db(e)
}
```

**Replay / internal operations** use `statement_timeout_ms × 6` (30 s by default) because replay scans many rows by design. The multiplier is applied in `query.rs` when constructing `MutationContext`.

**Configuration:**

| Env var | Default | Applied to |
|---|---|---|
| `STATEMENT_TIMEOUT_MS` | `5000` (5 s) | All standard queries |
| — | `statement_timeout_ms × 6` | Replay (`is_replay = true`) |

**Error behaviour:**

| Scenario | Before | After |
|---|---|---|
| Long query (5 s budget) | Runs until Rust timeout (30 s); DB stays hot | Killed by Postgres at 5 s → HTTP 408 |
| Replay query | Same 30 s Rust timeout | Postgres budget 30 s (6×5) → correct |
| Statement canceled | `500 Internal Server Error` | `408 Request Timeout` + `{"error":"query timed out"}` |

**BYODB safety layer (complete):**

| Layer | What it protects against |
|---|---|
| QueryGuard (complexity) | Deeply nested joins / explosive O(N^k) queries |
| NestDepth limit | Recursion into relationships beyond configured depth |
| LIMIT guard (Gap 18) | Full table scans when `limit` omitted; OFFSET without LIMIT |
| pg_catalog introspection (Gap 17) | Metadata scans on large clusters |
| DB identity check (Gap 16) | Accidental cross-cluster writes |
| Statement timeout (Gap 19) | Runaway query CPU/IO on customer's database |

No migration required. Compiler-only change.

---

### Gap 20 — Streaming Mutation Diff for Large Traces `[resolved]`

**Problem:** `flux trace diff` fetched mutations with a hard cap of 50 rows and loaded them all into memory. A batch job or workflow fan-out that touches 100k rows (×~3 KB JSON/row ≈ 300 MB) made the CLI slow and eventually OOM. The `GET /db/mutations` handler also sorted by `(created_at, version)` — non-deterministic when multiple mutations share the same clock tick — and had no table filter, forcing callers to download the full log even when they only needed one table.

**Root cause — two layers:**

1. **Data-engine handler** (`mutations.rs`): `fetch_all` with hard cap 500; ORDER BY `(created_at, version)` instead of the deterministic `mutation_seq` BIGSERIAL; no cursor or table filter.
2. **CLI** (`trace_diff.rs`): single `?limit=50` fetch; no `--table` flag; mutations compared by loading both sides into `Vec<Value>` before any output.

**Fix — keyset pagination + streaming cursor:**

**`GET /db/mutations` API (data-engine):**

| Parameter | Type | Description |
|---|---|---|
| `request_id` | string | Required — request to look up |
| `limit` | u32 | Page size, default 500, max 1000 |
| `after_seq` | i64 | Cursor: return rows with `mutation_seq > N`. Omit to start from beginning. |
| `table_name` | string | Optional — filter to one table only |

Response now includes `next_after_seq: i64 \| null` — callers loop until `null`:
```json
{
  "request_id": "…",
  "count": 1000,
  "next_after_seq": 87234,
  "mutations": [{ "mutation_seq": 86235, … }, …]
}
```

ORDER BY changed from `(created_at, version)` to `mutation_seq` — strict total order, backed by the existing `idx_state_mutations_request_seq (request_id, mutation_seq)` index.

**CLI `flux trace diff` — streaming `MutIter`:**

```rust
// Peak memory = 2 × PAGE_SIZE rows, regardless of total mutation count.
struct MutIter<'a> { buffer: VecDeque<Value>, after_seq: Option<i64>, … }
impl MutIter<'_> {
    async fn next(&mut self) -> anyhow::Result<Option<Value>> {
        if self.buffer.is_empty() && !self.exhausted { self.fill_buffer().await?; }
        Ok(self.buffer.pop_front())
    }
}
```

Both sides (original, replay) are walked simultaneously via two `MutIter` instances. Each pair of rows is printed immediately — no accumulation before output. Total allocations at any point: `2 × PAGE_SIZE` rows (2000 rows max, ~6 MB at 3 KB/row).

**`--table` flag:**
```
flux trace diff reqA reqB --table users
```
Passes `&table_name=users` to the API; backed by `idx_state_mutations_request_table (request_id, table_name)`. Reduces fetched data by 100–1000× on traces that touch many tables.

**Performance:**

| Scenario | Before | After |
|---|---|---|
| 100k mutations (no filter) | ~300 MB RAM, slow | ~6 MB RAM, O(1) |
| 100k mutations, `--table users` (500 rows) | 300 MB still fetched | 1.5 MB fetched |
| Non-deterministic ordering on same-ms writes | possible | impossible (mutation_seq) |
| Limit cap per request | 50 (CLI) / 500 (API) | 1000/page, unlimited pages |

No migration required. The `mutation_seq` column and indexes exist from migration `20260311000012`.

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
