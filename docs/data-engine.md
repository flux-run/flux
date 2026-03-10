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
8. [Configuration Reference](#configuration-reference)
9. [Deployment Notes](#deployment-notes)
10. [Potential Improvements](#potential-improvements)

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
2. Wraps the compiled SQL in `SELECT COALESCE(json_agg(row_to_json("_r")), '[]') FROM (...) AS "_r"` so the result is always a JSON array.
3. Binds all parameters via `PgArguments` (type-safe, never string-interpolated).
4. Commits the transaction.

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

User tables live in **project-scoped schemas** named `t_{tenant_slug}_{project_slug}_{db_name}`.

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

## Potential Improvements

Below are areas identified for review and enhancement:

### Security
- [ ] The `INTERNAL_SERVICE_TOKEN` defaults to a known static string (`fluxbase_secret_token`) — ensure `INTERNAL_SERVICE_TOKEN` is always set in production deployments.
- [ ] Consider adding request signing (HMAC) between API ↔ Data Engine in addition to the shared token, so compromised token alone is insufficient.
- [ ] Audit `validate_identifier` coverage — ensure it handles Unicode edge cases for schema/table names.

### Performance
- [ ] **Policy cache eviction** uses a `RwLock<HashMap>` with prefix scanning on writes — `O(n)` over all cached policies. Consider sharding the cache by tenant or switching to a Moka cache (consistent with schema/plan layers).
- [ ] **Plan cache** only covers `SELECT`. INSERT/UPDATE/DELETE are recompiled every request. Even a short-lived cache for mutation templates would help write-heavy workloads.
- [ ] **Batched executor** fetches child rows with separate queries per level. A `COPY`-based bulk fetch or a single lateral join with unnest could reduce round-trips.
- [ ] No connection pool size configuration is exposed (`DATABASE_URL` only). Add `DB_MAX_CONNECTIONS` + `DB_MIN_CONNECTIONS` env vars.

### Reliability
- [ ] **Cron `next_run_at`** is updated even when dispatch fails. A failed job schedule silently advances. Consider tracking failure counts and adding a `status` column to `cron_jobs`.
- [ ] **Event worker retry** needs a dead-letter mechanism. Events that exhaust retries should move to a `dead_letter` state (not just remain in `pending`/`failed`).
- [ ] **Workflow execution** does not handle step timeout — a step that never returns from the runtime leaves the execution stuck in `running` forever. Add a `stepped_at` timestamp and a timeout reaper.
- [ ] The workflow worker polls every 2 s unconditionally. A LISTEN/NOTIFY mechanism (Postgres pub/sub) would eliminate idle polling, improving latency and reducing DB load.

### Observability
- [ ] The `x-request-id` header is propagated to the runtime for hooks and events, but is not attached to Postgres query spans. Consider using `SET LOCAL application_name` or a Postgres comment to correlate DB traces.
- [ ] Complexity scores and cache hit/miss rates are logged at `debug` level only. Consider emitting structured metrics (Prometheus counters) for production dashboards.
- [ ] The `/db/debug` handler introspects engine state but its contents are not documented — review what it exposes and whether it should require elevated auth.

### Features
- [ ] **Computed columns** are injected as raw SQL expressions without sandboxing. Any expression stored in `column_metadata` is executed as-is. Consider a safe expression whitelist or a separate evaluation sandbox.
- [ ] **File columns** only support presigned S3 operations. No image resizing, CDN invalidation, or lifecycle policies are managed by the engine.
- [ ] **Relationships** are limited to foreign-key style joins. Many-to-many pivot tables require manual relationship definitions; there is no automatic pivot detection.
- [ ] No **soft-delete** or **audit trail** built into the engine — add a standard `deleted_at` column convention or a `fluxbase_internal.mutations` audit log.
- [ ] **Schema versioning** — `20260308000018_schema_versions.sql` exists on the API side. Consider whether the data engine should store per-table schema versions to support safe migrations (add column without downtime).
