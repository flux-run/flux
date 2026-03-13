# Flux — Full Specification

> **The backend framework where every execution is a record.**
> One binary. One port. One database. Every request fully traceable.

---

## Table of Contents

1. [What Flux Is](#1-what-flux-is)
2. [Architecture](#2-architecture)
3. [Project Structure](#3-project-structure)
4. [Functions](#4-functions)
5. [The ctx Object](#5-the-ctx-object)
6. [flux generate — Code Generation](#6-flux-generate--code-generation)
7. [Gateway — Validation & Routing](#7-gateway--validation--routing)
8. [Database](#8-database)
9. [Queue](#9-queue)
10. [Agents](#10-agents)
11. [Secrets](#11-secrets)
12. [Middleware](#12-middleware)
13. [Cron](#13-cron)
14. [Observability & Execution Recording](#14-observability--execution-recording)
15. [Debugging CLI](#15-debugging-cli)
16. [flux generate in Detail](#16-flux-generate-in-detail)
17. [Multi-Language Support](#17-multi-language-support)
18. [Self-Hosting & Deployment](#18-self-hosting--deployment)
19. [CLI Reference](#19-cli-reference)
20. [Implementation Plan](#20-implementation-plan)

---

## 1. What Flux Is

Flux is a **self-hosted backend framework where every execution is a record**.

Every function invocation automatically captures:
- Timing spans through every layer (gateway → runtime → DB → queue)
- Database mutations with before/after state for every write
- External HTTP calls and queue pushes
- The exact git SHA of deployed code
- Full request input and response output

All linked by a single `request_id`.

This enables:

```bash
flux why abc-123        # root cause in 10 seconds
flux trace abc-123      # full distributed waterfall
flux incident replay    # re-run with real production data
flux bug bisect         # find the commit that broke it
```

**The promise:** Production debugging should be deterministic, not guesswork.

### What Flux Is Not

- Not a managed cloud service (self-hosted, your infrastructure)
- Not an observability bolt-on (recording is built into the runtime, not an SDK)
- Not a rewrite-everything framework (strangler-fig migration path supported)

### Adoption Path

Developers don't migrate everything at once. They add one function, see `flux why` work on it, and migrate gradually.

```
Week 1:  One new endpoint written in Flux
         → flux why works on it immediately

Month 1: 20% of endpoints on Flux
         → team is sold on the debugging

Month 6: 80% migrated
         → old backend shrinking naturally
```

---

## 2. Architecture

### Single Binary. One Port. One Database.

All five modules run in a single Rust binary on port `:4000`:

```
┌─────────────────────────────────────────────────────┐
│                  Flux Server (:4000)                │
│                                                     │
│  ┌──────────┐  ┌─────────┐  ┌────────────────────┐ │
│  │ Gateway  │→ │ Runtime │→ │   Data Engine      │ │
│  │          │  │         │  │                    │ │
│  │ routing  │  │ Deno V8 │  │ mutations + hooks  │ │
│  │ auth     │  │ Wasmtime│  │ policy engine      │ │
│  │ rate lim │  │         │  │ cron               │ │
│  │ validate │  └────┬────┘  └────────────────────┘ │
│  └──────────┘       │                              │
│                ┌────▼────┐  ┌────────────────────┐ │
│                │  Queue  │  │       API          │ │
│                │         │  │                    │ │
│                │ workers │  │ registry + secrets │ │
│                │ retries │  │ traces + logs      │ │
│                └─────────┘  └────────────────────┘ │
└─────────────────────────────────────────────────────┘
                         │
              ┌──────────▼──────────┐
              │     PostgreSQL      │
              │  (single database)  │
              │                     │
              │  flux.*    system   │
              │  public.*  user     │
              └─────────────────────┘
```

### Module Responsibilities

| Module | Port (multi-service) | Responsibility |
|--------|---------------------|----------------|
| **Gateway** | 8081 | Public edge: routing, auth, rate limiting, JSON Schema validation, trace root |
| **Runtime** | 8083 | Function execution: Deno V8 (TypeScript), Wasmtime (WASM), isolate pool |
| **Data Engine** | 8082 | DB proxy: query execution, mutation recording, hooks, cron, policy enforcement |
| **Queue** | 8084 | Async jobs: polling, retries, dead-letter, visibility timeout |
| **API** | 8080 | Control plane: function registry, secrets, routes, trace retrieval |

### In-Process Communication

In the monolith (`server` binary), all modules communicate via **in-process function calls** — no HTTP overhead between modules. Each module exposes a trait:

```rust
pub trait RuntimeDispatch: Send + Sync {
    async fn execute(&self, req: ExecuteRequest) -> ExecuteResponse;
}

pub trait ApiDispatch: Send + Sync {
    async fn write_log(&self, entry: LogEntry);
    async fn get_bundle(&self, function_id: Uuid) -> Bundle;
    async fn get_secrets(&self, project_id: Uuid) -> Secrets;
}

pub trait DataEngineDispatch: Send + Sync {
    async fn query(&self, req: QueryRequest) -> QueryResponse;
}

pub trait QueueDispatch: Send + Sync {
    async fn push(&self, job: JobRequest) -> JobId;
}

pub trait AgentDispatch: Send + Sync {
    async fn run(&self, req: AgentRequest) -> AgentResponse;
}
```

### Database Schemas

Two Postgres schemas:

- **`flux.*`** — Flux system tables (owned by Flux, never touched by user code)
- **`public.*`** — User application tables (created by `flux db push`)

User functions see only `public.*`. System services see `flux.*` and `public.*`.

---

## 3. Project Structure

```
my-app/
├── flux.toml                    # project manifest — single config file
├── functions/                   # one directory per function = one POST endpoint
│   ├── create_user/
│   │   ├── index.ts             # TypeScript (Deno V8)
│   │   └── function.json        # JSON Schema for input/output
│   ├── send_email/
│   │   ├── index.ts
│   │   └── function.json
│   └── process_payment/
│       ├── main.rs              # Rust → compiled to WASM
│       ├── Cargo.toml
│       └── function.json
├── middleware/
│   └── auth.ts                  # defineMiddleware()
├── schemas/                     # raw SQL files — source of truth for DB
│   ├── users.sql
│   └── orders.sql
├── agents/                      # YAML agent definitions
│   └── support.yaml
├── .flux/                       # generated artifacts (gitignored)
│   ├── generated/               # auto-generated ctx types (never edit)
│   │   ├── ctx.ts
│   │   ├── ctx_types.rs
│   │   ├── ctx.go
│   │   └── ctx.py
│   ├── pgdata/                  # embedded Postgres data (flux dev)
│   └── build/                   # compiled WASM artifacts
└── .env.local                   # secrets for local dev (gitignored)
```

### flux.toml

```toml
[project]
name    = "my-app"
version = "1"

[dev]
port            = 4000
hot_reload      = true
reload_debounce = 150    # ms

[deploy]
target = "docker"        # local | docker | kubernetes

[limits]
timeout_ms  = 30000
memory_mb   = 128

[observability]
sampling_rate     = 1.0   # 1.0 = record every request
slow_span_ms      = 500   # flag spans slower than this
retention_days    = 90

[middleware]
global = ["auth"]         # middleware applied to all functions
```

---

## 4. Functions

Every directory under `functions/` automatically becomes a `POST /{name}` endpoint.

**Routing is POST-only.** This is intentional — all functions behave like webhook receivers, which makes them composable and language-agnostic.

### TypeScript Function

```ts
// functions/create_user/index.ts
import { defineFunction } from "@fluxbase/functions"

export default defineFunction({
  handler: async (input, ctx) => {
    const user = await ctx.db.users.insert({
      data: { name: input.name, email: input.email }
    })

    await ctx.queue.push("send_welcome_email", { userId: user.id })

    ctx.log.info(`Created user ${user.id}`)
    return { user }
  }
})
```

### Rust Function (WASM)

```rust
// functions/process_payment/main.rs
use flux_sdk::prelude::*;

#[flux_function]
async fn handler(input: ProcessPaymentInput, ctx: Ctx) -> Result<ProcessPaymentOutput> {
    let order = ctx.db().orders().find_one(FindOneArgs {
        where_: OrdersWhere { id: Some(input.order_id) }
    }).await?;

    let charge = ctx.functions().stripe_charge(StripeChargeInput {
        amount: order.total,
        currency: "usd".to_string(),
    }).await?;

    ctx.db().orders().update(UpdateArgs {
        where_: OrdersWhere { id: Some(order.id) },
        data: UpdateOrder { status: Some("paid".to_string()) },
    }).await?;

    Ok(ProcessPaymentOutput { charge_id: charge.id })
}
```

### Go Function (WASM)

```go
// functions/send_report/main.go
package main

import "github.com/fluxbase/flux-go/sdk"

func Handler(input SendReportInput, ctx sdk.Ctx) (SendReportOutput, error) {
    users, err := ctx.DB().Users().Find(sdk.FindArgs{})
    if err != nil { return SendReportOutput{}, err }

    for _, user := range users {
        ctx.Queue().Push("send_email", SendEmailInput{
            To:      user.Email,
            Subject: "Weekly Report",
        })
    }

    return SendReportOutput{Sent: len(users)}, nil
}
```

### function.json — Input/Output Schema

Every function has a `function.json` defining its input/output as **JSON Schema**. This is the single source of truth for validation — used by the Gateway to reject invalid requests before the function runs.

```json
{
  "name": "create_user",
  "description": "Creates a new user account",
  "input": {
    "type": "object",
    "properties": {
      "name":  { "type": "string", "minLength": 1, "maxLength": 100 },
      "email": { "type": "string", "format": "email" }
    },
    "required": ["name", "email"],
    "additionalProperties": false
  },
  "output": {
    "type": "object",
    "properties": {
      "user": { "$ref": "#/definitions/User" }
    },
    "required": ["user"]
  }
}
```

**Why JSON Schema over Zod:**
- Every language has a JSON Schema validator
- Gateway can validate in Rust before the function starts
- Same schema used for TypeScript types, Rust structs, Go types — generated by `flux generate`
- Not tied to any runtime or language ecosystem

### Validation Flow

```
POST /create_user { name: "", email: "not-an-email" }
         ↓
Gateway loads route → fetches function.json input schema
         ↓
Validates request body against JSON Schema
         ↓
❌ Invalid → 400 { "error": "validation_failed", "details": [...] }
   Function never starts. Nothing recorded.
         ↓
✅ Valid → forward to runtime with validated payload
   Function receives pre-validated input. No validation code needed.
```

---

## 5. The ctx Object

`ctx` is the **single interface to everything Flux provides**. It is **fully typed to your app** — not a generic API. Types are generated by `flux generate` from your actual schema, functions, secrets, and agents.

### Full Interface

```ts
interface Ctx {
  // Database — typed to your actual tables
  db: {
    users: TableCtx<User, InsertUser, UpdateUser, UsersWhere>
    orders: TableCtx<Order, InsertOrder, UpdateOrder, OrdersWhere>
    // ... every table in your schemas/
  }

  // Secrets — typed to your actual secret names
  secrets: {
    OPENAI_KEY: string
    STRIPE_SECRET: string
    // ... every key in your secret store
  }

  // Functions — typed to your actual functions + their schemas
  functions: {
    create_user(input: CreateUserInput): Promise<CreateUserOutput>
    send_email(input: SendEmailInput): Promise<SendEmailOutput>
    // ... every function in your functions/
  }

  // Agents — typed to your actual agents
  agents: {
    support(input: { message: string }): Promise<{ reply: string }>
    // ... every agent in your agents/
  }

  // Queue
  queue: {
    push<T extends keyof Functions>(
      fn: T,
      payload: FunctionInput<T>,
      opts?: { delay?: number; idempotencyKey?: string }
    ): Promise<void>
  }

  // Logging (structured, linked to request_id)
  log: {
    info(message: string, meta?: object): void
    warn(message: string, meta?: object): void
    error(message: string, meta?: object): void
  }

  // Error handling
  error(code: string, message: string, details?: object): never

  // Request context
  requestId: string
  headers: Record<string, string>
  user?: AuthUser          // set by middleware
}
```

### TableCtx Interface

Every table exposes a consistent typed API:

```ts
interface TableCtx<Row, Insert, Update, Where> {
  find(args?: { where?: Where; limit?: number; offset?: number; orderBy?: OrderBy }): Promise<Row[]>
  findOne(args: { where: Where }): Promise<Row | null>
  insert(args: { data: Insert }): Promise<Row>
  insertMany(args: { data: Insert[] }): Promise<Row[]>
  update(args: { where: Where; data: Update }): Promise<Row>
  updateMany(args: { where: Where; data: Update }): Promise<Row[]>
  delete(args: { where: Where }): Promise<void>
  deleteMany(args: { where: Where }): Promise<void>
  count(args?: { where?: Where }): Promise<number>
  query(sql: string, params?: unknown[]): Promise<Row[]>  // escape hatch
}
```

### Where Clause Operators

```ts
type WhereField<T> =
  | T                               // exact match
  | { eq: T }                       // equals
  | { neq: T }                      // not equals
  | { gt: T; lt?: T }               // greater than
  | { gte: T; lte?: T }             // greater than or equal
  | { like: string }                // SQL LIKE
  | { ilike: string }               // case-insensitive LIKE
  | { in: T[] }                     // IN (...)
  | { nin: T[] }                    // NOT IN (...)
  | { is_null: boolean }            // IS NULL / IS NOT NULL
```

### Why ctx Is Generated, Not Hand-Written

The ctx object knows:
- Every table name and its columns (from `schemas/*.sql`)
- Every function name and its input/output (from `functions/*/function.json`)
- Every secret key name (from secret store — names only, not values)
- Every agent name and its interface (from `agents/*.yaml`)

This is fetched at `flux generate` time and written to `.flux/generated/`. The runtime then injects the real values (secret values, DB connection) at execution time.

**Secret values are never in generated files.** Only key names.

---

## 6. flux generate — Code Generation

`flux generate` is the command that makes ctx fully typed. It reads your app and generates typed bindings for every language.

```bash
flux generate
# Reads:   schemas/*.sql → table types
#          functions/*/function.json → function input/output types
#          secret store → secret key names
#          agents/*.yaml → agent interfaces
#
# Writes:  .flux/generated/ctx.ts         TypeScript
#          .flux/generated/ctx_types.rs   Rust (WASM functions)
#          .flux/generated/ctx.go         Go (WASM functions)
#          .flux/generated/ctx.py         Python (WASM functions)
```

### What Gets Generated

#### TypeScript (`.flux/generated/ctx.ts`)
Complete typed `Ctx` interface + all table types, function signatures, secret keys.

#### Rust (`.flux/generated/ctx_types.rs`)
Structs for all tables, input/output types, secrets struct.

```rust
// Auto-generated — do not edit
pub struct SecretsCtx {
    pub openai_key: String,
    pub stripe_secret: String,
}

pub struct DbCtx {
    pub users: UsersTableCtx,
    pub orders: OrdersTableCtx,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct User {
    pub id: Uuid,
    pub name: String,
    pub email: String,
    pub created_at: DateTime<Utc>,
}
```

#### Go (`.flux/generated/ctx.go`)
Structs and interface types for all tables and functions.

#### Python (`.flux/generated/ctx.py`)
Dataclasses for all tables and functions.

### Hot Reload

In `flux dev`, `flux generate` runs automatically whenever:
- A file in `schemas/` changes
- A `function.json` changes
- A secret is added/removed
- An agent YAML changes

Debounced at 150ms (configurable in `flux.toml`).

---

## 7. Gateway — Validation & Routing

The gateway is the **only public-facing component**. It is responsible for:

1. **Route resolution** — in-memory snapshot of all routes, updated via Postgres `LISTEN/NOTIFY`
2. **Authentication** — API key, JWT, or none (per-route config)
3. **Rate limiting** — token bucket per `(route_id, client_ip)`
4. **JSON Schema validation** — validates request body against `function.json` input schema
5. **Trace root creation** — generates `request_id`, writes to `flux.gateway_trace_requests`
6. **Dispatch** — forwards validated request to runtime

### Request Pipeline

```
Incoming request
      │
      ▼
[1] Content-length guard (reject > max_body_size)
      │
      ▼
[2] Route resolution (in-memory HashMap lookup)
      │ 404 if not found
      ▼
[3] CORS preflight handling
      │
      ▼
[4] Authentication (api_key | jwt | none)
      │ 401 if invalid
      ▼
[5] Rate limiting (token bucket)
      │ 429 if exceeded
      ▼
[6] Body read + JSON Schema validation
      │ 400 if schema violation (function never starts)
      ▼
[7] Trace root write (fire-and-forget, async)
      │ request_id generated here
      ▼
[8] Dispatch to runtime (in-process)
      │
      ▼
Response
```

### Validation at Gateway (Not in Functions)

**All input validation happens at step [6], before the function starts.**

This means:
- Invalid requests are rejected cheaply (no V8 isolate startup, no WASM instantiation)
- Functions receive pre-validated input — no validation code needed in user functions
- Consistent validation across all languages (TypeScript, Rust, Go, Python — same JSON Schema)
- Validation errors are uniform across all functions

### Route Snapshot

Routes are stored in an in-memory `HashMap<(Method, Path), RouteEntry>`.

```rust
struct RouteEntry {
    function_id: Uuid,
    function_version: String,
    input_schema: JsonSchema,    // loaded at snapshot time
    auth_config: AuthConfig,
    rate_limit: RateLimitConfig,
    middleware: Vec<MiddlewareId>,
}
```

Updated via Postgres `LISTEN/NOTIFY` on the `flux_routes_changed` channel — zero polling, immediate updates on deploy.

---

## 8. Database

### User Writes Functions, Flux Owns the Writes

User functions call `ctx.db.*` which goes through the **Data Engine**. The Data Engine:

1. Compiles the operation to SQL
2. Reads the before-state (for updates/deletes) in the same transaction
3. Executes the SQL
4. Writes the mutation record (before/after) in the **same transaction**
5. Returns the result

The mutation is **atomic with the data write**. Rolling back the write rolls back the mutation log. No race conditions. No eventual consistency.

### Why Flux Controls Writes

This is intentional and non-negotiable. Only by controlling writes can Flux:

- Record before/after state atomically
- Enable `flux state history` (full row version history)
- Enable `flux state blame` (last writer per column)
- Enable `flux incident replay` (replay with side-effects disabled)
- Enforce row-level security policies

### DB Operations

```ts
// Find multiple rows
const users = await ctx.db.users.find({
  where: { status: "active" },
  orderBy: { created_at: "desc" },
  limit: 20
})

// Find one row
const user = await ctx.db.users.findOne({
  where: { email: "john@example.com" }
})

// Insert
const user = await ctx.db.users.insert({
  data: { name: "John", email: "john@example.com" }
})

// Update
const updated = await ctx.db.users.update({
  where: { id: userId },
  data: { status: "inactive" }
})

// Delete
await ctx.db.users.delete({
  where: { id: userId }
})

// Raw SQL (escape hatch)
const result = await ctx.db.users.query(
  "SELECT * FROM users WHERE created_at > $1",
  [cutoffDate]
)
```

### Schemas

Database schema is defined in raw SQL files in `schemas/`:

```sql
-- schemas/users.sql
CREATE TABLE users (
  id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name       TEXT NOT NULL,
  email      TEXT NOT NULL UNIQUE,
  status     TEXT NOT NULL DEFAULT 'active',
  created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX idx_users_email  ON users(email);
CREATE INDEX idx_users_status ON users(status);
```

```bash
flux db push      # apply schemas to database
flux db diff      # show what would change
flux generate     # regenerate ctx types from schema
```

### Mutation Recording

Every write is recorded in `flux.state_mutations`:

```sql
-- Every INSERT/UPDATE/DELETE
flux.state_mutations (
  id           BIGSERIAL PRIMARY KEY,
  request_id   UUID NOT NULL,      -- links to the request that caused it
  span_id      UUID,               -- links to the span within that request
  table_name   TEXT NOT NULL,
  record_pk    JSONB NOT NULL,     -- primary key of affected row
  operation    TEXT NOT NULL,      -- INSERT | UPDATE | DELETE
  before_state JSONB,              -- NULL for INSERT
  after_state  JSONB,              -- NULL for DELETE
  changed_fields TEXT[],           -- for UPDATE: which fields changed
  actor_id     TEXT,               -- user/service that made the change
  created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
)
```

---

## 9. Queue

### Pushing Jobs

```ts
// From any function — enqueue for async processing
await ctx.queue.push("send_welcome_email", {
  userId: user.id,
  email: user.email
})

// With options
await ctx.queue.push("process_payment", { orderId }, {
  delay: 5000,                     // wait 5 seconds before processing
  idempotencyKey: `pay-${orderId}` // prevent duplicate jobs
})
```

### Worker Functions

A worker is just a function. Its directory name matches the job name:

```ts
// functions/send_welcome_email/index.ts
import { defineFunction } from "@fluxbase/functions"

export default defineFunction({
  handler: async (input, ctx) => {
    const user = await ctx.db.users.findOne({ where: { id: input.userId } })
    // send email via external HTTP call
    await fetch("https://api.sendgrid.com/...", { ... })
    return { sent: true }
  }
})
```

Queue jobs execute through the **same runtime pipeline** as HTTP requests. Every job execution produces a full execution record with the same `request_id` chain.

### Job Lifecycle

```
PENDING → RUNNING → COMPLETED
                  ↘ FAILED → RETRY (up to max_attempts)
                                   ↘ DEAD_LETTER
```

| Property | Default | Configurable |
|----------|---------|--------------|
| Max attempts | 3 | Per job |
| Backoff | Exponential (1s → 4s → 16s) | Fixed in implementation |
| Visibility timeout | 5 minutes | Global config |
| Poll interval | 200ms | `WORKER_POLL_INTERVAL_MS` |
| Max concurrent workers | 50 | `WORKER_CONCURRENCY` |

### Idempotency

```ts
// This job will only be enqueued once, even if called multiple times
await ctx.queue.push("charge_card", { orderId }, {
  idempotencyKey: `charge-${orderId}`
})
```

Duplicate pushes with the same `idempotencyKey` are silently ignored.

---

## 10. Agents

Agents are **LLM-driven orchestrators** defined as YAML. They use your functions as tools. Every step is automatically traced.

### Agent Definition

```yaml
# agents/support.yaml
name: support
model: gpt-4o-mini
system: |
  You are a customer support agent. Use the provided tools to help users
  resolve their issues. Always look up the user's account before responding.

tools:
  - get_user
  - create_ticket
  - update_ticket
  - send_email

max_turns: 10
temperature: 0.3

llm_url: https://api.openai.com/v1/chat/completions
llm_secret: OPENAI_KEY    # secret name from your secret store

rules:
  - before: send_email
    require: create_ticket   # must create ticket before sending email
  - tool: get_user
    max_calls: 3             # can only call get_user 3 times per run
```

### Running Agents

```ts
// From any function
const result = await ctx.agents.support({
  message: "I was charged twice for order #1234"
})
// result: { reply: "I've found your account and created ticket #5678..." }
```

### What Gets Recorded

Every agent run produces spans in the execution record:

```
agent_start         → "Running support agent, turn 1"
  tool_call         → get_user({ email: "john@example.com" })
  tool_result       → { id: "u_123", name: "John" }
  tool_call         → create_ticket({ userId: "u_123", ... })
  tool_result       → { id: "t_456", status: "open" }
  llm_call          → turn 2, model: gpt-4o-mini, tokens: 312
  tool_call         → send_email({ to: "john@...", subject: "..." })
  tool_result       → { sent: true }
agent_end           → "Completed in 3 turns, 847 tokens"
```

Full debuggable with `flux agent trace <request-id>`.

---

## 11. Secrets

Secrets are encrypted at rest (AES-256-GCM), versioned, and injected at runtime.

### Setting Secrets

```bash
flux secrets set OPENAI_KEY sk-...
flux secrets set STRIPE_SECRET sk_live_...
flux secrets list
flux secrets delete OPENAI_KEY
```

### Accessing Secrets

```ts
// ctx.secrets is typed to your actual secret keys
const key = ctx.secrets.OPENAI_KEY    // string
const stripe = ctx.secrets.STRIPE_SECRET
```

**Secrets are never:**
- Stored in generated files (only key names are generated)
- Logged or included in execution records
- Returned in error messages
- Visible in `flux trace` output

### Local Development

```bash
# .env.local (gitignored)
OPENAI_KEY=sk-test-...
STRIPE_SECRET=sk_test_...
```

`flux dev` reads `.env.local` and injects values as secrets locally.

---

## 12. Middleware

Middleware runs before the function handler, in the same isolate, with access to the full `ctx` object.

### Defining Middleware

```ts
// middleware/auth.ts
import { defineMiddleware } from "@fluxbase/functions"

export default defineMiddleware({
  handler: async (ctx) => {
    const token = ctx.headers["authorization"]?.replace("Bearer ", "")
    if (!token) ctx.error("unauthorized", "Missing token")

    const user = await ctx.db.users.findOne({
      where: { token }
    })
    if (!user) ctx.error("unauthorized", "Invalid token")

    ctx.user = user   // attach to ctx for downstream use
  }
})
```

### Assigning Middleware

**Global** (all functions):
```toml
# flux.toml
[middleware]
global = ["auth"]
```

**Per function group:**
```toml
# flux.toml
[[middleware.groups]]
name = "admin"
middleware = ["auth", "require_admin"]
functions = ["delete_user", "ban_user", "export_data"]
```

**Per function:**
```json
// functions/create_user/function.json
{
  "middleware": ["auth"]
}
```

---

## 13. Cron

Scheduled functions defined in `flux.toml`:

```toml
[[cron]]
function = "cleanup_sessions"
schedule = "0 * * * *"     # every hour

[[cron]]
function = "send_digest"
schedule = "0 9 * * 1"     # every Monday 9am
```

Cron jobs execute through the same runtime pipeline — fully traced, debuggable with `flux why`.

---

## 14. Observability & Execution Recording

### The Execution Record

Every request — HTTP, queue job, cron job, agent run — produces an **execution record** automatically. No instrumentation code needed.

```
execution_record {
  request_id      UUID        // unique identifier, propagated everywhere
  function_name   TEXT
  code_sha        TEXT        // git SHA of deployed code
  method          TEXT
  path            TEXT
  input           JSONB       // request body
  output          JSONB       // response body
  status          INT         // HTTP status code
  duration_ms     INT
  error           JSONB       // structured error if any
  created_at      TIMESTAMPTZ
}
```

### Spans

Every operation within a request emits a span:

```
spans: [
  { type: "gateway",    source: "gateway",     message: "route resolved",    delta_ms: 0.2  }
  { type: "start",      source: "runtime",     message: "execution_start",   delta_ms: 1.1  }
  { type: "tool",       source: "data_engine", message: "users.findOne",     delta_ms: 3.4  }
  { type: "event",      source: "runtime",     message: "user found",        delta_ms: 0.1  }
  { type: "tool",       source: "data_engine", message: "orders.insert",     delta_ms: 8.2  }
  { type: "tool",       source: "queue",       message: "push send_email",   delta_ms: 0.8  }
  { type: "end",        source: "runtime",     message: "execution_end",     delta_ms: 0.3  }
]
```

All spans have: `request_id`, `span_id`, `parent_span_id`, `source`, `span_type`, `message`, `timestamp`, `duration_ms`.

### Automatic Detections

The trace endpoint automatically detects:

| Detection | Condition | Action |
|-----------|-----------|--------|
| **Slow span** | `delta_ms >= slow_threshold` (default 500ms) | Flagged in `flux trace` |
| **N+1 query** | Same table queried ≥3 times in one request | Warning with table name |
| **Missing index** | Table scan detected in query plan | Suggestion in `flux trace` |
| **Timeout** | Function exceeds timeout | Structured timeout span |

### Storage Schema

```
flux.gateway_trace_requests  — request envelopes (append-only)
flux.platform_logs           — all spans (append-only)
flux.state_mutations         — DB mutations with before/after (append-only)
```

All three tables are **append-only**. Written asynchronously after the response is sent (fire-and-forget). Never block the request path.

### Retention

Configurable per project in `flux.toml`:

```toml
[observability]
retention_days = 90    # auto-pruned after this
```

---

## 15. Debugging CLI

### flux why `<request-id>`

The core debugging command. Answers: *what went wrong and why?*

```
$ flux why 550e8400

✗  POST /checkout → checkout   (3200ms · 500)

ROOT CAUSE
  Stripe API timeout after 10000ms
  span: tool/stripe_charge  (checkout/index.ts:42)
  code: a93f42c

STATE AT FAILURE
  orders  INSERT  id=7f3a  total=99.00  status="pending"
             → order was created but never paid

FIX SUGGESTION
  External call timeout. Add retry with backoff.
  Test with: flux incident replay 550e8400
```

**How it works:**
1. Fetches all spans for `request_id` from `flux.platform_logs`
2. Fetches all mutations for `request_id` from `flux.state_mutations`
3. Pattern-matches the error span against known failure patterns
4. Reads mutation before/after to show data state at failure time
5. Suggests fix based on pattern

### flux trace `<request-id>`

Full distributed trace as a waterfall with timings.

```
$ flux trace 550e8400

POST /checkout  550e8400  3200ms  ✗

  SPAN                             SOURCE        DELTA
  ─────────────────────────────────────────────────────
  route resolved                   gateway         0.2ms
  ├ execution_start                runtime         1.1ms
  ├ users.findOne (id=u_123)       data_engine     3.4ms
  ├ orders.insert                  data_engine     8.2ms
  ├ stripe_charge ←── SLOW        external       3180ms  ⚠
  └ execution_end                  runtime         0.3ms

⚠  1 slow span (>500ms)
⚠  No N+1 queries detected
```

### flux trace debug `<request-id>`

Interactive step-through of a request in the terminal. Navigate spans with arrow keys, inspect inputs/outputs.

### flux trace diff `<id-a>` `<id-b>`

Compare two requests span-by-span. Shows exactly what changed between a working and failing request.

### flux state history `<table>` `--id <row-id>`

Full version history of a single row — every mutation, who made it, which request caused it.

```
$ flux state history orders --id 7f3a1234

orders · id=7f3a1234

  #   TIME              OP      CHANGED        REQUEST
  ──────────────────────────────────────────────────────
  1   2026-03-13 08:01  INSERT  —              550e8400
  2   2026-03-13 08:04  UPDATE  status         a1b2c3d4
  3   2026-03-13 08:11  UPDATE  status, total  f9e8d7c6
```

### flux state blame `<table>`

Shows the last writer per row and per column.

### flux incident replay `<from>`.`<to>`

Re-runs all requests from a time window against your current code:
- Side-effects disabled (emails, webhooks, external HTTP → mocked)
- Database writes execute normally
- Every replay produces a new execution record
- Useful for testing a fix against real production traffic

```bash
flux incident replay "2026-03-13T08:00".."2026-03-13T09:00"
flux incident replay 550e8400          # single request
```

### flux bug bisect `--request <id>`

Binary-searches your git history to find the commit that caused a request to fail:

```bash
flux bug bisect --request 550e8400 --good v1.2.0 --bad v1.3.0
# Deploys intermediate commits, replays the request, checks for error
# → "Bug introduced in commit a93f42c: increase stripe timeout"
```

### flux tail

Live stream of incoming requests with inline error summaries.

```bash
flux tail              # all requests
flux tail --errors     # errors only
flux tail --fn create_user   # specific function
```

---

## 16. flux generate in Detail

### What It Reads

| Source | What it extracts |
|--------|-----------------|
| `schemas/*.sql` | Table names, column names, types, nullability, defaults |
| `functions/*/function.json` | Function names, input/output JSON Schemas |
| Secret store (API call) | Secret key names (not values) |
| `agents/*.yaml` | Agent names, input/output interfaces |

### Generation Pipeline

```
flux generate
  │
  ├─ 1. Parse schemas/*.sql
  │     → build TableDef[] (name, columns, types, PKs, indexes)
  │
  ├─ 2. Parse functions/*/function.json
  │     → build FunctionDef[] (name, inputSchema, outputSchema)
  │
  ├─ 3. Fetch secret keys from API
  │     → build SecretsDef (key names only)
  │
  ├─ 4. Parse agents/*.yaml
  │     → build AgentDef[] (name, inputSchema, outputSchema)
  │
  ├─ 5. Generate TypeScript → .flux/generated/ctx.ts
  ├─ 6. Generate Rust       → .flux/generated/ctx_types.rs
  ├─ 7. Generate Go         → .flux/generated/ctx.go
  └─ 8. Generate Python     → .flux/generated/ctx.py
```

### SQL Type Mapping

| PostgreSQL | TypeScript | Rust | Go | Python |
|-----------|-----------|------|-----|--------|
| `uuid` | `string` | `Uuid` | `uuid.UUID` | `str` |
| `text` | `string` | `String` | `string` | `str` |
| `integer` | `number` | `i32` | `int32` | `int` |
| `bigint` | `number` | `i64` | `int64` | `int` |
| `boolean` | `boolean` | `bool` | `bool` | `bool` |
| `timestamptz` | `string` | `DateTime<Utc>` | `time.Time` | `datetime` |
| `jsonb` | `unknown` | `serde_json::Value` | `interface{}` | `Any` |
| `text[]` | `string[]` | `Vec<String>` | `[]string` | `list[str]` |
| nullable | `T \| null` | `Option<T>` | `*T` | `Optional[T]` |

---

## 17. Multi-Language Support

Flux supports two execution engines:

| Engine | Languages | Use Case |
|--------|-----------|---------|
| **Deno V8** | TypeScript, JavaScript | Default for most backends |
| **Wasmtime** | Rust, Go, C, Python, Zig, AssemblyScript | Performance-critical, existing code |

### Language-Specific SDKs

Each language has a generated `ctx` + a thin SDK for the host imports:

| Language | Package |
|----------|---------|
| TypeScript | `@fluxbase/functions` |
| Rust | `flux-sdk` crate |
| Go | `github.com/fluxbase/flux-go` |
| Python | `fluxbase` PyPI package |

### WASM Execution

WASM functions are compiled AOT by Wasmtime with Cranelift:

```bash
# Rust function
flux build functions/process_payment  # cargo build --target wasm32-wasi

# Go function
flux build functions/send_report      # GOARCH=wasm GOOS=wasip1 go build

# Python function
flux build functions/analyze_data     # componentize-py
```

Built artifacts stored in `.flux/build/`. Deployed as a bundle alongside TypeScript functions.

**WASM performance:**
- Cranelift AOT compilation (compile once, cache 256 entries)
- ~10µs instantiation per request
- Linear memory isolation (each request gets its own memory)
- No filesystem access (sandboxed)
- Fuel-based CPU metering (prevent infinite loops)

### Bringing Existing Code

**No rewrite needed.** Developers compile their existing Rust/Go service to WASM and deploy it to Flux. The function gets `flux why` immediately.

```bash
# Existing Rust service function → WASM → Flux
cargo build --target wasm32-wasi --release
flux deploy functions/existing_handler
```

---

## 18. Self-Hosting & Deployment

### Development

```bash
flux dev
# starts embedded Postgres at .flux/pgdata/
# starts flux server on :4000
# watches functions/ for changes, hot-reloads
# runs flux generate on schema/function changes
```

### Docker Compose

```yaml
# docker-compose.yml
services:
  flux:
    image: ghcr.io/fluxbase/flux:latest
    ports: ["4000:4000"]
    environment:
      DATABASE_URL: postgresql://postgres:postgres@db:5432/flux
    depends_on: [db]

  db:
    image: postgres:16
    volumes: ["pgdata:/var/lib/postgresql/data"]
```

```bash
docker compose up
```

### Kubernetes

```bash
helm repo add fluxbase https://charts.fluxbase.dev
helm install flux fluxbase/flux \
  --set database.url="postgresql://..."
```

### Scaling

All services are stateless. Scale horizontally:

```bash
docker compose up --scale gateway=4 --scale runtime=8
```

### Production Checklist

- [ ] TLS termination at load balancer (Flux speaks HTTP internally)
- [ ] `INTERNAL_SERVICE_TOKEN` set (service-to-service auth)
- [ ] `/internal/*` routes not exposed externally
- [ ] `DATABASE_URL` points to managed Postgres (RDS, Supabase, Neon, Fly)
- [ ] S3/R2 configured for function bundle storage
- [ ] Retention policy set (`observability.retention_days`)
- [ ] Health check: `GET /health` → `{ "status": "ok" }`

---

## 19. CLI Reference

### Project

```bash
flux init <name>         # Create new project
flux dev                 # Start local dev server
flux build               # Build all functions (compile WASM, bundle TS)
flux build --fn <name>   # Build one function
flux generate            # Regenerate ctx types
```

### Deploy

```bash
flux deploy              # Deploy all functions
flux deploy --fn <name>  # Deploy one function
flux rollback --fn <name> --to <sha>  # Rollback to previous version
```

### Database

```bash
flux db push             # Apply schemas/ to database
flux db diff             # Show pending schema changes
```

### Secrets

```bash
flux secrets set <key> <value>
flux secrets delete <key>
flux secrets list
```

### Debugging

```bash
flux why <request-id>                        # Root cause analysis
flux trace <request-id>                      # Full span waterfall
flux trace debug <request-id>                # Interactive step-through
flux trace diff <id-a> <id-b>               # Compare two requests
flux tail                                    # Live request stream
flux tail --errors                           # Errors only
flux state history <table> --id <row-id>     # Row version history
flux state blame <table>                     # Last writer per row
flux incident replay <from>..<to>            # Replay time window
flux incident replay <request-id>            # Replay single request
flux bug bisect --request <id> --good <sha> --bad <sha>
```

### Agents

```bash
flux agent list
flux agent trace <request-id>
flux agent why <request-id>
```

### Logs

```bash
flux logs                      # Recent logs
flux logs --fn <name>          # Specific function
flux logs --follow             # Stream live
flux errors                    # Per-function error summary
```

---

## 20. Implementation Plan

### What Exists (Built)

**Rust services:**
- ✅ Gateway — routing, auth, rate limiting, trace root, request dispatch
- ✅ Runtime — Deno V8 isolate pool, WASM pool (Wasmtime), span emission, bundle cache
- ✅ Data Engine — query execution, mutation recording (before/after), hooks, cron
- ✅ Queue — DB polling, job lifecycle, retries, dead-letter, idempotency
- ✅ API — function registry, secrets, routes, log ingestion, trace retrieval
- ✅ Server — in-process monolith composing all five modules
- ✅ Agent — LLM loop, tool dispatch, step recording
- ✅ CLI — `flux why`, `flux trace`, `flux trace diff`, `flux trace debug`, `flux tail`, `flux deploy`

**TypeScript:**
- ✅ `@fluxbase/functions` — `defineFunction()`, `defineMiddleware()`
- ✅ Dashboard — management UI (integrated in server)
- ✅ Frontend — marketing site + docs (Next.js)

**Infrastructure:**
- ✅ All database schemas and migrations
- ✅ Docker Compose (dev + production)

### Critical Gaps to Fix (Phase 0 Completion)

These are blocking `flux why` from working fully end-to-end:

| # | Gap | File | Effort |
|---|-----|------|--------|
| 1 | **UPDATE before-state is NULL** | `data-engine/src/executor/db_executor.rs` | 3h |
| 2 | **Queue logs not in platform_logs** | `queue/src/worker/executor.rs` | 2h |
| 3 | **parent_span_id not stored in platform_logs** | `api/src/logs/routes.rs` + runtime | 4h |
| 4 | **Runtime lifecycle spans missing** | `runtime/src/execute/runner.rs` | 2h |
| 5 | **Query params not stored in trace root** | `gateway/src/handlers/dispatch.rs` | 1h |
| 6 | **Delete unused `flux.state_mutations` API table** | `schemas/api/` | 30m |

### Next Phase (After Phase 0)

1. **`flux generate`** — full code generation pipeline (TypeScript + Rust + Go + Python)
2. **`flux dev`** — embedded Postgres, zero-config local setup, hot reload
3. **`flux.toml` parser** — project manifest reading in CLI
4. **JSON Schema validation at Gateway** — replace Zod-based validation
5. **Language templates** — starter templates for TypeScript, Rust, Go, Python functions
6. **`flux db push` / `flux db diff`** — schema management commands
7. **`flux state history`** — CLI command (infrastructure exists, command missing)
8. **Dashboard** — connect to live trace/mutation APIs

### Architecture Decisions (Final)

| Decision | Choice | Reason |
|----------|--------|--------|
| Validation | JSON Schema (Gateway) | All languages, no runtime dependency |
| Runtimes | Deno V8 + Wasmtime | TypeScript default + multi-language |
| Database | PostgreSQL only | Single source of truth, mutation recording |
| Queue | DB-backed (no Redis/Kafka) | No external deps, simpler ops |
| Tracing | Built into runtime | Automatic, no SDK needed |
| Generated types | Per-language from schema | Full type safety in every language |
| Binary | Single monolith | No inter-service HTTP, one deployment |
