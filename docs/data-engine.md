# Data Engine

> **Internal architecture doc.** This describes the Data Engine service
> implementation for contributors. For user-facing docs, see
> [framework.md §10](framework.md#10-database).

---

## Overview

| Property | Value |
|---|---|
| Service name | `flux-data-engine` |
| Role | Database queries, mutation recording, hooks, events, cron |
| Tech | Rust, Axum, SQLx, PostgreSQL |
| Default port | `:8082` |
| Exposed to internet | No — receives traffic from Runtime and API only |

The Data Engine sits between user functions and Postgres. No user traffic
reaches it directly — everything flows through Runtime (via `ctx.db`) or
API (for schema management), authenticated via `X-Service-Token`.

```
Runtime :8083
     │  ctx.db.users.insert(data)  →  POST /db/query
     ▼
Data Engine :8082
     ├── Query compiler (typed wrapper → SQL)
     ├── Policy engine (row/column level security)
     ├── Mutation recorder (before/after JSONB)
     ├── Hook engine (pre/post mutation function triggers)
     ├── Event system (fan-out to webhooks, functions, queues)
     ├── Workflow engine (step-advance on events)
     └── Cron worker (schedule-based job firing)
     │
     ▼
PostgreSQL (user application database)
```

---

## Core invariant

**Every database write must go through the Data Engine.** This is non-negotiable.
The Data Engine captures before/after state for every mutation, links it to the
`request_id`, and writes it to `execution_mutations`. This is the foundation of:

- `flux why` — sees what data changed during a failed request
- `flux state history` — full version history of any row
- `flux state blame` — which request last modified a row
- `flux incident replay` — deterministic re-execution with recorded state

Any write that bypasses the Data Engine is invisible to all debugging tools.

---

## Query compilation

`ctx.db.users.findMany({ where: { email: { eq: "ada@acme.com" } } })` is not
an ORM call. The Runtime sends it as a structured JSON request to the Data
Engine, which compiles it to SQL:

```
Input:  { table: "users", operation: "select", filters: [{ column: "email", op: "eq", value: "ada@acme.com" }] }
Output: SELECT * FROM users WHERE email = $1   (params: ["ada@acme.com"])
```

This is why schemas are raw SQL but queries feel typed — `flux generate` reads
`information_schema` and emits TypeScript types, while the Data Engine compiles
typed accessor calls into SQL at runtime.

`ctx.db.query(sql, params)` bypasses the typed wrapper but still goes through
the Data Engine, so mutations are still recorded.

---

## Mutation recording

Every INSERT, UPDATE, DELETE is recorded atomically (same transaction as the
data change):

```sql
INSERT INTO execution_mutations (
  request_id, table_name, operation, row_id,
  before_state, after_state, span_id, created_at
) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW());
```

- `before_state` is `NULL` for INSERT
- `after_state` is `NULL` for DELETE
- `span_id` links to the exact execution span that caused the write
- Append-only — no UPDATE or DELETE on mutation records

---

## BYODB (Bring Your Own Database)

The Data Engine supports separate databases for platform metadata and
application data:

| Database | Stores |
|---|---|
| Platform DB | execution records, spans, mutations, policies, hooks, events |
| User DB | application tables (users, orders, etc.) |

This allows Flux to operate as an execution + observability layer on top of
an existing Postgres database without requiring data migration.

---

## Policy engine

Row and column level security evaluated before every query:

- Policies defined via API service
- Evaluated per-request based on `ctx.user` claims
- Applied transparently — user code never sees policy logic

---

## Hook engine

Pre/post mutation hooks that invoke Flux functions:

```
Before INSERT on users → call validate_user function
After INSERT on users  → call send_welcome_email function
```

Hooks execute synchronously in the same request context (same `request_id`),
so they appear in the execution record.

---

## Event system

Mutations can emit events that fan out to:
- Webhook endpoints
- Flux functions
- Queue jobs

Events are persisted before delivery (at-least-once guarantee).

---

## Cron worker

The Data Engine runs a background cron worker that fires scheduled jobs:

- Schedules stored in Postgres
- Evaluated every minute
- Jobs dispatched to Runtime via Queue service
- Each cron execution produces an execution record

---

## Configuration

| Env var | Default | Description |
|---|---|---|
| `PORT` | `8082` | HTTP listen port |
| `DATABASE_URL` | — | Platform Postgres (metadata, mutations, logs) |
| `USER_DATABASE_URL` | — | User Postgres (application tables) — optional, defaults to `DATABASE_URL` |
| `INTERNAL_SERVICE_TOKEN` | — | Service-to-service auth |
| `RUNTIME_URL` | `http://localhost:8083` | For hook execution |
| `QUEUE_URL` | `http://localhost:8084` | For event/cron dispatch |

---

*Source: `data-engine/src/`. For the database spec, see
[framework.md §10](framework.md#10-database).*
