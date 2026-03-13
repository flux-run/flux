# Flux Framework

> **Naming note:** CNCF Flux (Fluxcd) is a widely-used GitOps tool.
> The name "Flux" is a working title. Before public launch, evaluate:
> `fluxrun`, `fluxkit`, `fluxdb`, or keep `flux` and own the SEO fight.
> Decision deadline: before first npm publish and CLI binary release.

> Flux is a backend framework where every execution is a record.
> Every function call captures inputs, outputs, database mutations, external calls,
> and spans — automatically. Production bugs become reproducible with one command.
>
> Flux is Git for backend execution.

---

## Table of Contents

0. [Why Flux Exists](#0-why-flux-exists)
1. [What Flux Is](#1-what-flux-is)
2. [Standalone & Self-Hosted](#2-standalone--self-hosted)
3. [Execution Record](#3-execution-record)
4. [Architecture](#4-architecture)
5. [Golden Path](#5-golden-path)
6. [Project Structure & flux.toml](#6-project-structure--fluxtoml)
7. [Local Dev — flux dev](#7-local-dev--flux-dev)
8. [Functions & The ctx Object](#8-functions--the-ctx-object)
9. [Middleware](#9-middleware)
10. [Database](#10-database)
11. [Secrets](#11-secrets)
12. [Error Model](#12-error-model)
13. [Type Generation](#13-type-generation)
14. [Queue](#14-queue)
15. [Cron](#15-cron)
16. [Agents](#16-agents)
17. [Testing](#17-testing)
18. [Observability & Debugging](#18-observability--debugging)
19. [Auth](#19-auth)
20. [Build & Deploy](#20-build--deploy)
21. [Self-Hosted Deployment](#21-self-hosted-deployment)
22. [CLI Reference](#22-cli-reference)
23. [Implementation Phases](#23-implementation-phases)

---

## 0. Why Flux Exists

Backend execution is ephemeral. A function runs. It reads from a database.
It calls Stripe. It pushes a job. It returns. Then it's gone.

When something breaks, you get:

- Scattered logs across Datadog, Sentry, CloudWatch
- No idea what the function was called with
- No record of which DB row was read or mutated
- A local repro that doesn't match production

Debugging becomes: `grep logs → check Sentry → look at DB → guess → repeat`.

**Flux inverts this.** Every execution is a record — inputs, outputs, every DB
mutation (before and after), every external call, every span. Stored automatically.
Queryable. Replayable.

```bash
flux why a3f9d2b1
# → Function: create_user
# → Error: CONFLICT on users.email
# → DB mutation: INSERT INTO users failed — duplicate key (email = alice@example.com)
# → Fix: check for existing user before inserting
```

**Why choose Flux over Express + Prisma + BullMQ?**

You could wire up Express, Prisma, BullMQ, a cron library, OpenTelemetry, Sentry,
and a logging service yourself. That's 6 dependencies, 3 config files, and zero
connection between them. When a production bug hits, you're jumping between 4
dashboards trying to reconstruct what happened.

Flux gives you one framework where functions, database, queue, cron, and full
execution history are integrated from the start. The tracing isn't bolted on —
it's the runtime's primary output. You write a function, and every invocation is
automatically recorded end-to-end.

---

## 1. What Flux Is

**One sentence:** Flux is a backend framework where every execution is a record.

```
HTTP Request
     │
     ▼
 ExecutionRecord
  ├── spans[]          → flux trace <id>
  ├── db_mutations[]   → flux state history
  ├── external_calls[] → flux why
  └── input + code_sha → flux incident replay
```

Functions are the input. `ExecutionRecord`s are the output. The framework ensures
every request passes through the runtime, which enables deterministic recording.
Execution history is not optional observability — it is the runtime's primary output.

Flux provides:

- **One project structure** — learn once, apply everywhere
- **One local runtime** — `flux dev` mirrors production exactly
- **JS/TS functions** via Deno — secure, fast, no `node_modules`
- **Execution recording** — every request traced, every mutation logged
- **Deterministic replay** — reproduce any production request locally
- **Database + queue + cron + agents** — integrated, not bolted on
- **An observability CLI** that replaces your APM — without setup

Flux runs **entirely locally without any cloud services**.

**Scope boundary:** Flux owns things it needs to record intelligently —
functions, database, queue, agents. Everything else (storage, email, payments,
third-party APIs) is a function that uses an SDK. The generic `ExternalCall`
trace is enough for those — no special primitives needed.

**Flux never owns your data.** Application databases belong to you. Flux only
records execution metadata (inputs, outputs, spans, mutation diffs) for debugging
and replay.

---

## 2. Standalone & Self-Hosted

Flux is a standalone open-source framework. There is no managed cloud service.
You run it locally, in Docker, or on Kubernetes — on your own infrastructure.

```
Flux (framework)
  flux dev        → local dev server
  flux build      → compile artifacts
  flux deploy     → push to any target
  flux test       → test runner
  flux trace      → execution records
  flux why        → root cause
```

| Deploy target | What it means |
|---|---|
| `local` | Hot-swap into running `flux dev` |
| `docker` | Build a `FROM flux/server` image |
| `k8s` | Generate Kubernetes manifests |

No vendor lock-in. Your Postgres, your data, your network.

---

## 3. Execution Record

The Execution Record is the core primitive. Everything else exists to produce,
query, and replay it.

```typescript
interface ExecutionRecord {
  request_id:    string;
  function_id:   string;
  function_name: string;
  code_sha:      string;          // git commit of deployed code
  deployed_at:   string;

  input:         JsonValue;
  output:        JsonValue | null;
  error:         FluxError | null;

  started_at:    string;
  duration_ms:   number;

  spans:           ExecutionSpan[];
  db_mutations:    DbMutation[];
  external_calls:  ExternalCall[];

  runtime:       "deno";
  project_id:    string;
}

interface ExecutionSpan {
  span_id:     string;
  parent_id:   string | null;
  service:     "gateway" | "runtime" | "data-engine" | "queue";
  span_type:   "route_match" | "cache_hit" | "execution" | "db_query" | "function_invoke" | "agent_step" | "event";
  message:     string;
  started_at:  string;
  duration_ms: number;
  metadata:    Record<string, JsonValue>;
}

interface DbMutation {
  table:     string;
  operation: "INSERT" | "UPDATE" | "DELETE";
  row_id:    string;
  before:    JsonValue | null;   // null for INSERT
  after:     JsonValue | null;   // null for DELETE
  query_ms:  number;
}

interface ExternalCall {
  kind:        "http" | "queue_push" | "function_invoke" | "agent_step";
  target:      string;
  input:       JsonValue;
  output:      JsonValue | null;
  duration_ms: number;
  error:       string | null;
}
```

### What the execution record enables

| Command | Uses |
|---|---|
| `flux trace <id>` | Render `spans` as waterfall |
| `flux why <id>` | Parse `error` + `db_mutations` + `external_calls` → root cause |
| `flux incident replay <id>` | Re-execute with same `input` + `code_sha`, mock externals |
| `flux trace diff <a> <b>` | Diff two records field by field |
| `flux bug bisect` | Binary search `code_sha` values over recorded executions |
| `flux test --trace` | Assert on `spans` and `db_mutations`, not just return values |
| `flux state history <table>` | Pivot `db_mutations` by table + row |

### Storage

Four Postgres tables, all linked by `request_id`:

| Table | Contains |
|---|---|
| `execution_records` | Root row: function, input/output, error, timing, code_sha |
| `execution_spans` | Distributed trace spans |
| `execution_mutations` | DB mutations with before/after JSONB |
| `execution_calls` | External HTTP calls, function invocations, queue pushes |

---

## 4. Architecture

Flux is a **single binary**. One process, one port, everything in-process.

```
my-app/
├── flux.toml
├── functions/
├── schemas/
└── tests/

$ flux dev → http://localhost:4000  ← the only port

  One binary, five modules:
    Gateway      routing, rate limiting, CORS, auth
    Runtime      Deno V8 execution, secrets, agent dispatch
    API          function registry, logs, schema management
    Data Engine  DB queries, mutation recording, hooks, cron
    Queue        async jobs, retries, dead letter

  All modules communicate in-process — no HTTP between them.
  Every request → x-request-id → ExecutionRecord → queryable via CLI
```

Rust + Axum. Single binary, single port. The Runtime uses `deno_core`
for V8 isolate execution. Database is Postgres. All modules share one
`PgPool` and one tokio runtime. Scaling is horizontal: run more copies
of the same binary behind a load balancer.

```
Load Balancer
  ├── flux-server (instance 1)  ← full stack, port 4000
  ├── flux-server (instance 2)  ← full stack, port 4000
  └── flux-server (instance 3)  ← full stack, port 4000
          │
      Postgres (shared)
```

Every module is stateless. Postgres holds all state. No service discovery,
no inter-service URLs, no independent scaling of individual components.

---

## 5. Golden Path

Project to production debugging in under 5 minutes:

```bash
# 1. Create
flux init my-app && cd my-app

# 2. Start (all services, hot reload, local Postgres)
flux dev

# 3. Edit functions/hello/index.ts → saves → reloads in <200ms

# 4. Push DB schema
flux db push

# 5. Deploy
flux deploy                     # deploys to default target from flux.toml

# 6. Debug
flux why <request-id>           # root cause in 10 seconds
```

**Constraints:**
- `flux dev` works with zero config — no `.env`, no Docker setup required
- First invocation error prints exactly which file to create
- No cloud account or external service required

---

## 6. Project Structure & flux.toml

### Layout

```
my-app/
├── flux.toml               project manifest
├── functions/
│   ├── hello/
│   │   └── index.ts
│   ├── create_user/
│   │   └── index.ts
│   └── send_email/
│       └── index.ts
├── middleware/
│   └── auth.ts
├── schemas/
│   ├── users.sql
│   └── orders.sql
├── agents/
│   └── support.ts
├── tests/
│   ├── create_user.test.ts
│   └── fixtures/
│       └── users.sql
└── .env.local              local secrets (gitignored)
```

### flux.toml

```toml
[project]
name    = "my-app"
version = "0.1.0"

[dev]
port               = 4000
hot_reload         = true
reload_debounce_ms = 100

[deploy]
target = "local"              # "local" | "docker" | "k8s"

[limits]
timeout_ms = 30000
memory_mb  = 128

[observability]
# Sample rate for successful requests. Errors always recorded at 100%.
# Default is 1.0 — every execution is a record. That's the product promise.
# At high traffic (>1k rps), recording every request adds ~2ms of write latency
# per request and ~50 bytes/span in Postgres. Reduce to 0.1 only when you've
# measured the cost and decided the trade-off is worth it.
record_sample_rate = 1.0

[middleware]
# See §9 for middleware definition and execution model.
global = ["middleware/auth.ts"]

[middleware.groups]
public    = []
protected = ["middleware/auth.ts"]
admin     = ["middleware/auth.ts", "middleware/require_admin.ts"]
```

**Opinionated defaults:**
- Deploy target defaults to `local`, not cloud
- Errors are always recorded — not configurable
- One config file, everything in `flux.toml`

---

## 7. Local Dev — flux dev

`flux dev` starts the Flux binary and a managed local Postgres.
One process, one port, watches for changes, hot-reloads.

```
flux dev
  ├─ Start Postgres       (auto-managed, data at .flux/pgdata/)
  ├─ Start flux-server    → http://localhost:4000  (single process)
  ├─ Watch functions/     → on change: build + invalidate cache
  └─ Print: Flux running at http://localhost:4000
```

### Local mode

In local mode (`flux dev`): skip tenant resolution, disable JWT auth,
route directly to the in-process runtime. Same routing logic, just
bypassed tenant lookup.

### Hot reload

On file change in `functions/`:
1. Detect change (FSEvents)
2. Build artifact (`flux build <name>`)
3. Deploy to local (`flux deploy <name>`)
4. Invalidate runtime caches (`POST /internal/cache/invalidate`)
5. Print: `✓ hello reloaded (234ms)`

### Local Postgres

`flux dev` auto-manages a local Postgres instance:
- Uses `pg_embed` or a bundled binary
- Data stored at `.flux/pgdata/`
- Port auto-assigned, written to `.flux/dev.env`
- Persisted between runs, destroyed with `flux dev --clean`

No Docker required. No manual database setup. Just `flux dev`.

---

## 8. Functions & The ctx Object

### Routing

Every function directory under `functions/` becomes an HTTP endpoint automatically:

```
functions/hello/index.ts       → POST http://localhost:4000/hello
functions/create_user/index.ts → POST http://localhost:4000/create_user
functions/send_email/index.ts  → POST http://localhost:4000/send_email
```

All function endpoints are `POST`. The function name is the route. No route
files, no decorators, no manual registration. Drop a directory in `functions/`,
it becomes an endpoint.

POST-only is intentional: webhooks from Stripe, GitHub, and most third-party
services send POST, so inbound integrations work without config. For GET health
checks or static responses, the gateway exposes `GET /health` natively — this
is not a function, it's a gateway route. If a future use case requires GET
endpoints (e.g., OAuth callbacks), it will be added as a `method` field in
`flux.json`, not as a routing DSL.

The gateway reads the function registry from the API service and builds a
`RouteSnapshot` mapping names to runtime targets. In local mode this happens
at startup + on every hot reload.

### Per-function config — flux.json

Each function directory can include an optional `flux.json` to override
project-level defaults:

```json
{
  "runtime": "deno",
  "timeout": "10s",
  "memory_mb": 256,
  "retries": 2,
  "middleware": "protected",
  "description": "Creates a new user account"
}
```

Omitted fields inherit from `flux.toml` `[limits]`. If no `flux.json` exists,
all defaults apply.

**Precedence order** (highest wins): `defineFunction()` fields → `flux.json` →
`flux.toml [limits]`. If `flux.json` sets `timeout: "10s"` and
`defineFunction({ timeout: "30s" })`, the function-level `30s` wins. Code is
closest to the function, so code wins.

### Defining a function

Every function uses `defineFunction()`. No raw handlers.

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";

export default defineFunction({
  name: "create_user",
  input:  z.object({ name: z.string(), email: z.string().email() }),
  output: z.object({ id: z.string() }),
  handler: async ({ input, ctx }) => {
    const user = await ctx.db.users.insert(input);
    await ctx.queue.push("send_welcome_email", { user_id: user.id });
    return { id: user.id };
  },
});
```

### The `ctx` object

Every handler receives `ctx`. This is the single interface to all Flux capabilities.
No imports, no client instantiation, no connection strings.

```typescript
interface FluxContext {
  // Identity
  requestId: string;              // UUID, propagated through entire execution
  functionName: string;

  // Database — typed from schemas/ via flux generate
  db: {
    [table: string]: {
      insert(data: object): Promise<Row>;
      update(id: string, data: object): Promise<Row>;
      delete(id: string): Promise<void>;
      findById(id: string): Promise<Row | null>;
      findOne(query: QueryFilter): Promise<Row | null>;
      findMany(query?: QueryFilter): Promise<Row[]>;
      query(sql: string, params?: any[]): Promise<Row[]>;
    };
  };

  // Queue
  queue: {
    push(fn: string, payload: object, opts?: {
      delay?: string;                // "5m", "1h", "24h"
      idempotencyKey?: string;
    }): Promise<void>;
  };

  // Cross-function calls (traced, same request_id)
  function: {
    invoke(name: string, input: object): Promise<any>;
  };

  // Agents — run an agent, which uses functions as tools
  agent: {
    run(name: string, input: object): Promise<any>;
  };

  // Secrets — loaded from env, never logged
  secrets: {
    get(key: string): string | undefined;
  };

  // Error helper — throws structured error, stops execution
  error(code: number, error: string, message?: string): never;

  // Request metadata
  headers: Headers;
  user?: any;                     // set by auth middleware

  // Logging — automatically attached to execution record
  log: {
    info(msg: string, data?: object): void;
    warn(msg: string, data?: object): void;
    error(msg: string, data?: object): void;
  };
}
```

**How `ctx.db` works under the hood:**
- `ctx.db.users.insert(data)` → in-process call to the Data Engine module
- Data Engine executes the SQL, captures before/after state as a `DbMutation`
- Mutation is written to `execution_mutations` linked by `request_id`
- This is why Flux requires its own DB layer — mutation recording needs control
  over every write

**Three ways to query, one source of truth.** Schemas are raw SQL files in
`schemas/`. `flux generate` reads `information_schema` from the live database
and emits TypeScript types — that's where `Row`, `QueryFilter`, and the typed
table accessors come from. At runtime, `ctx.db.users.findMany({ where: ... })`
is not an ORM — it's a thin typed wrapper that compiles to SQL inside the Data
Engine. `ctx.db.query(sql, params)` is the escape hatch for anything the
wrapper can't express (joins, CTEs, window functions). Both paths go through
the Data Engine, so both are recorded. The mental model: SQL schemas are the
authoritative definition, `flux generate` derives types, typed accessors are
convenience, raw SQL is always available.

**How `ctx.function.invoke` works:**
- HTTP call through the gateway with `X-Service-Token`
- Same `x-request-id` propagated → traces are linked
- Invocation appears in `external_calls` as `kind: "function_invoke"`

### Function metadata

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `name` | string | directory name | Function identifier |
| `timeout` | string | `"30s"` | Max execution time |
| `retries` | int | `0` | Auto-retry on transient error |
| `memory_mb` | int | `128` | Memory limit |
| `concurrency` | int | unlimited | Max parallel executions |
| `cron` | string | — | Cron schedule (see §16) |
| `description` | string | — | Shown in `flux spec`, OpenAPI |

---

## 9. Middleware

Middleware runs before every function. Defined once, applied globally or per-group.

### Definition

```typescript
// middleware/auth.ts
import { defineMiddleware } from "@flux/functions";

export default defineMiddleware(async (ctx, next) => {
  const token = ctx.headers.get("authorization")?.replace("Bearer ", "");
  if (!token) return ctx.error(401, "UNAUTHORIZED", "Missing auth header");

  const user = await verifyJWT(token, ctx.secrets.get("JWT_SECRET")!);
  if (!user) return ctx.error(401, "INVALID_TOKEN", "Token expired");

  ctx.user = user;
  return next();
});
```

### Configuration

In `flux.toml`:

```toml
[middleware]
global = ["middleware/auth.ts"]

[middleware.groups]
public    = []                          # no auth
protected = ["middleware/auth.ts"]
admin     = ["middleware/auth.ts", "middleware/require_admin.ts"]
```

Per-function assignment in the function's directory `flux.json`:

```json
{ "middleware": "public" }
```

### Execution order

```
Gateway → Runtime → [middleware chain] → function handler
                      auth.ts
                      rate_limit.ts
                          └─▶ handler({ input, ctx })
```

- Same Deno isolate, shared `ctx`
- `ctx.user`, `ctx.metadata` survive from middleware into handler
- If middleware returns without calling `next()`, execution stops (short-circuit)

---

## 10. Database

Flux manages your application database. **One Postgres database per Flux
instance.** No ORM — SQL schemas, typed access via `ctx.db`.

### Schema files

```sql
-- schemas/users.sql
CREATE TABLE IF NOT EXISTS users (
  id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name       TEXT NOT NULL,
  email      TEXT NOT NULL UNIQUE,
  created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);
```

### Commands

```bash
flux db push       # apply schemas/*.sql to local or remote DB
flux db diff       # preview what SQL will run (never executes anything)
flux db migrate    # save diff as timestamped migration file
flux db seed       # apply tests/fixtures/*.sql
flux db reset      # drop + recreate + push + seed
```

### flux db diff

```bash
$ flux db diff

  +  CREATE TABLE orders (
  +    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  +    user_id UUID REFERENCES users(id),
  +    total NUMERIC NOT NULL
  +  );

  ~  ALTER TABLE users ADD COLUMN phone TEXT;

  Run `flux db push` to apply.
```

Compares `schemas/*.sql` (desired state) against `information_schema.columns`
(current state). Safe — never runs anything.

### Migration files

```
migrations/
  20260312000001_add_orders_table.sql
  20260312000002_add_users_phone.sql
```

Standard SQL migration files, run sequentially via `sqlx::migrate!`.

### Why Flux owns the DB layer

Flux can't record `DbMutation` before/after diffs without controlling writes.
This is why `ctx.db` exists instead of letting you use Prisma directly.
The Data Engine intercepts every write, captures the row state before and after,
and writes the mutation to `execution_mutations`. That's the foundation of
`flux why` and `flux state history`.

### Migrating from Prisma / Drizzle / raw pg

If you have an existing app, you cannot adopt Flux without moving writes to
`ctx.db`. This is the hardest migration cost. The recommended path:

1. **Start with new endpoints only.** Write new functions in Flux, keep existing
   code unchanged. Both can share the same Postgres database.
2. **Move reads first.** Replace `prisma.user.findMany()` with `ctx.db.users.findMany()`.
   Reads don't require mutation recording — this is a mechanical change.
3. **Move writes incrementally.** One table at a time, replace `prisma.user.create()`
   with `ctx.db.users.insert()`. Each table moved gains full execution history.
4. **Raw SQL escape hatch.** `ctx.db.query(sql, params)` runs arbitrary SQL
   through the Data Engine, so any query Prisma can express, Flux can run.

You don't need to rewrite your entire data layer on day one. But every write
that bypasses `ctx.db` is invisible to `flux why`.

### One database per instance

One Flux instance = one Postgres database. Application tables and Flux's internal
tables (`execution_records`, `execution_spans`, `execution_mutations`,
`execution_calls`) share the same database, same connection pool, same
`DATABASE_URL`.

This matches the single-binary philosophy: one binary, one port, one database.
Rails shipped with one database for 15 years — it's enough for 99% of apps.

If you genuinely need a second database (analytics warehouse, legacy system),
connect to it inside your function with a Postgres client. Flux doesn't manage
it, but Flux records the call as an `ExternalCall`:

```typescript
// functions/sync_analytics/index.ts
import pg from "pg";

export default defineFunction({
  name: "sync_analytics",
  handler: async ({ ctx }) => {
    const analytics = new pg.Pool({ connectionString: ctx.secrets.get("ANALYTICS_DB_URL") });
    const rows = await ctx.db.orders.findMany({ where: { synced: false } });
    await analytics.query("INSERT INTO order_events ...", rows);
    return { synced: rows.length };
  },
});
```

### No built-in storage

Flux does not provide a storage primitive (S3, file uploads, CDN). Storage is
a solved problem — use the SDK for your provider inside a function:

```typescript
// functions/upload_avatar/index.ts
import { S3Client, PutObjectCommand } from "@aws-sdk/client-s3";

export default defineFunction({
  name: "upload_avatar",
  handler: async ({ input, ctx }) => {
    const s3 = new S3Client({ region: ctx.secrets.get("AWS_REGION") });
    await s3.send(new PutObjectCommand({
      Bucket: ctx.secrets.get("S3_BUCKET"),
      Key: `avatars/${input.userId}`,
      Body: input.file,
    }));
    return { url: `https://${ctx.secrets.get("S3_BUCKET")}.s3.amazonaws.com/avatars/${input.userId}` };
  },
});
```

`flux trace` shows the S3 call, duration, and status — without Flux owning any
storage infrastructure. The rule: if Flux doesn't need to intercept the call to
provide debugging value beyond what `ExternalCall` already captures, it's not a
primitive.

---

## 11. Secrets

```bash
flux secrets set STRIPE_KEY sk_live_...
flux secrets get STRIPE_KEY
flux secrets list
flux secrets delete STRIPE_KEY
```

### Local dev

`.env.local` is loaded automatically by `flux dev`. Gitignored.

```
STRIPE_KEY=sk_test_...
DATABASE_URL=postgres://...
JWT_SECRET=...
```

### In functions

```typescript
const key = ctx.secrets.get("STRIPE_KEY");
```

Secrets are injected by the Runtime via the existing `SecretsClient` (LRU + TTL
cached). Never logged, never included in execution records.

---

## 12. Error Model

Every error across all services uses one structure:

```json
{
  "error":      "VALIDATION_ERROR",
  "message":    "name is required",
  "code":       400,
  "request_id": "a3f9d2b1-...",
  "violations": [
    { "field": "/name", "message": "required property 'name' not found" }
  ]
}
```

### Standard codes

| Code | HTTP | Meaning |
|------|------|---------|
| `INPUT_VALIDATION_ERROR` | 400 | Failed JSON Schema / Zod validation |
| `UNAUTHORIZED` | 401 | Missing or invalid auth token |
| `FORBIDDEN` | 403 | Auth OK, insufficient permissions |
| `NOT_FOUND` | 404 | Resource doesn't exist |
| `CONFLICT` | 409 | Duplicate / state conflict |
| `RATE_LIMITED` | 429 | Too many requests |
| `FUNCTION_ERROR` | 500 | Unhandled exception in function |
| `TIMEOUT` | 504 | Function exceeded timeout |
| `DEPENDENCY_ERROR` | 502 | External call failed |

### Throwing errors in functions

```typescript
if (!input.email.includes("@")) {
  return ctx.error(400, "INVALID_EMAIL", "Email address is not valid");
}
```

The `ctx.error()` helper throws a structured error that the runtime catches
and maps to the standard envelope. Same signature as in the FluxContext:
`ctx.error(httpCode, errorCode, message?)`.
JSON Schema validation runs **before** the function executes (Rust layer),
Zod validation runs inside the function.

---

## 13. Type Generation

```bash
flux generate
```

Produces `flux.d.ts` with types for everything:

```typescript
// flux.d.ts (generated — do not edit)

export namespace DB {
  interface users  { id: string; name: string; email: string; created_at: string; }
  interface orders { id: string; user_id: string; total: number; status: string; }
}

export namespace Functions {
  interface create_user { input: { name: string; email: string }; output: { id: string }; }
  interface send_email  { input: { to: string; subject: string }; output: { sent: boolean }; }
}

export interface FluxDB {
  users:  FluxTable<DB.users>;
  orders: FluxTable<DB.orders>;
}

export interface FluxFunctions {
  invoke(fn: "create_user", input: Functions.create_user["input"]): Promise<Functions.create_user["output"]>;
  invoke(fn: "send_email",  input: Functions.send_email["input"]):  Promise<Functions.send_email["output"]>;
}
```

### Data sources

| Type | Source |
|------|--------|
| DB tables | `information_schema.columns` via `GET /internal/introspect` |
| Function contracts | `input_schema` + `output_schema` via `GET /internal/introspect` |
| Secret keys | `secrets.key` via `GET /internal/introspect` |
| Agent definitions | Agent name, model, tool-functions via `GET /internal/introspect` |

All sources exposed by existing endpoints. `flux generate` calls one endpoint
and renders a `.d.ts` file.

---

## 14. Queue

### Pushing jobs

```typescript
await ctx.queue.push("send_email", {
  to: "alice@example.com",
  subject: "Your order shipped",
});

// With delay
await ctx.queue.push("send_reminder", payload, { delay: "24h" });

// With idempotency
await ctx.queue.push("charge_subscription", payload, {
  idempotencyKey: `charge_${userId}_${month}`,
});
```

### CLI

```bash
flux worker                    # start local queue worker
flux worker --concurrency 10   # control parallelism
flux queue list                # show pending/running/failed jobs
flux queue retry <job-id>      # retry a failed job
flux queue dead-letter         # list dead-letter jobs
```

Queue pushes are recorded in `external_calls` with `kind: "queue_push"`.
Failed jobs create their own execution records, queryable with `flux why`.

---

## 15. Cron

Attach a schedule directly to a function:

```typescript
export default defineFunction({
  name: "daily_report",
  cron: "0 0 * * *",
  handler: async ({ ctx }) => {
    const stats = await ctx.db.orders.findMany({
      where: { created_at: { gte: yesterday() } }
    });
    // ...
  },
});
```

```bash
flux cron list    # list active cron jobs + next run times
```

The `cron` field is parsed at deploy time. The Data Engine's cron worker fires
jobs through the Queue, which dispatches to Runtime. Each cron invocation
produces a normal execution record.

---

## 16. Agents

Agents are LLM-driven orchestrators that use your functions as tools.
Every function is automatically available as a tool — no separate registry.

### Defining an agent

```typescript
// agents/support.ts
import { defineAgent } from "@flux/functions";

export default defineAgent({
  name: "support_agent",
  model: "gpt-4",
  tools: ["lookup_user", "create_ticket", "send_email"],
  system: "You are a customer support agent. Look up the user, assess their issue, create a ticket, and send a confirmation email.",
});
```

`tools` is a list of function names. The agent calls them as tool invocations.
Each call produces a traced execution record — same as any other function call.

### Running an agent

From code:
```typescript
const result = await ctx.agent.run("support_agent", {
  message: "I can't log in to my account",
  user_email: "alice@example.com",
});
```

From CLI:
```bash
flux agent run support_agent --data '{"message": "I can't log in"}'
```

### How agents use functions

The agent runtime translates each `defineFunction()` into an LLM tool schema
automatically — `input` becomes the tool parameters, `output` becomes the
return type. No manual schema writing.

### Third-party integrations

Stripe, OpenAI, Resend, etc. are just functions that wrap an SDK:

```typescript
// functions/stripe_create_customer/index.ts
export default defineFunction({
  name: "stripe_create_customer",
  input: z.object({ email: z.string().email(), name: z.string() }),
  output: z.object({ customer_id: z.string() }),
  handler: async ({ input, ctx }) => {
    const stripe = new Stripe(ctx.secrets.get("STRIPE_KEY")!);
    const customer = await stripe.customers.create({ email: input.email, name: input.name });
    return { customer_id: customer.id };
  },
});
```

Now an agent can use `stripe_create_customer` as a tool. The integration is
just a function — traced, replayable, debuggable.

### Agent execution records

Every agent run produces an execution record with:
- Each LLM call as a span (`agent_step`)
- Each tool/function invocation as a child execution record
- The full conversation (system prompt + messages + tool calls + responses)

`flux trace` and `flux why` work on agent runs the same as any function.

---

## 17. Testing

```typescript
// tests/create_user.test.ts
import { test, expect, flux } from "@flux/testing";

test("create_user returns an id", async () => {
  const result = await flux.invoke("create_user", {
    name: "Alice",
    email: "alice@example.com",
  });
  expect(result.id).toBeDefined();
});

test("create_user rejects duplicate email", async () => {
  await flux.invoke("create_user", { name: "Alice", email: "dup@example.com" });
  await expect(
    flux.invoke("create_user", { name: "Alice", email: "dup@example.com" })
  ).rejects.toMatchObject({ error: "CONFLICT" });
});
```

### Trace assertions

Tests can assert on execution internals, not just return values:

```typescript
test("create_user emits user.created event", async () => {
  const { request_id } = await flux.invokeWithTrace("create_user", payload);
  const trace = await flux.trace(request_id);
  expect(trace.spans).toContainEqual(
    expect.objectContaining({ span_type: "event", message: "user.created" })
  );
});
```

### Running tests

```bash
flux test                      # run all tests
flux test --watch              # re-run on file change
flux test tests/create_user    # run one file
```

`flux test` automatically:
1. Starts `flux dev` if not running
2. Runs `flux db reset` → `flux db push` → `flux db seed`
3. Executes tests in parallel
4. Reports pass/fail with diff

### Fixtures

```sql
-- tests/fixtures/users.sql
INSERT INTO users (id, name, email) VALUES
  ('00000000-0000-0000-0000-000000000001', 'Test User', 'test@example.com');
```

---

## 18. Observability & Debugging

This is the defining feature. Every other framework bolts tracing on via
OpenTelemetry or a third-party APM. Flux records execution history as a
first-class runtime primitive — not optional, not a separate service.

### Commands

```bash
# Tracing
flux trace <request-id>                 # full distributed trace
flux trace <id> --flame                 # waterfall visualization
flux why <request-id>                   # root cause + fix suggestion
flux tail                               # live request stream
flux tail --function create_user        # filter by function
flux errors                             # per-function error summary
flux logs create_user --follow          # tail logs for a function

# State
flux state history users --id <uuid>    # field-by-field row history
flux state blame users                  # last writer per row

# Replay
flux incident replay <request-id>       # re-run with side effects suppressed
flux trace diff <id-a> <id-b>           # compare two executions
flux bug bisect --function <name> --good <sha> --bad <sha>
```

### Execution trace (automatic)

Every request produces this without instrumentation:

```
gateway.route_match          +0ms
  runtime.bundle_cache_hit   +2ms
  runtime.execution_start    +4ms
    function.ctx.log(...)    +5ms
    db.query.users           +6ms  (8ms, before/after captured)
    fn.stripe_create_customer +20ms (145ms, input/output captured)
  runtime.execution_end      +170ms

db_mutations: [{ table: users, op: UPDATE, before: {...}, after: {...} }]
external_calls: [{ kind: function_invoke, target: stripe_create_customer, ... }]
```

All spans linked by `x-request-id` + `x-parent-span-id`.

### Replay

`flux incident replay <id>` re-executes with the exact same input and code version.
Side effects are suppressed for safety:

| Call type | Replay behavior |
|---|---|
| DB reads | **Live** — reads current DB |
| DB writes | **Suppressed** (pass `--write` to allow) |
| HTTP calls | **Mocked** — returns recorded response |
| Queue pushes | **Suppressed** |
| Cross-function calls | **Mocked** — returns recorded output |
| `ctx.log()` / spans | **Live** — new record created with `replay: true` |

```bash
flux incident replay a3f9d2b1               # dry-run
flux incident replay a3f9d2b1 --write       # allow DB writes
flux incident replay a3f9d2b1 --live-http   # real outbound calls
```

Replay creates a new execution record tagged `replay: true` pointing to the
original. Compare with `flux trace diff <original> <replay>`.

### Local trace viewer

During `flux dev`, a visual trace is served at
`http://localhost:4000/trace/<id>` — a static HTML page rendering the
execution record as a clickable waterfall with mutations and external calls
annotated on each span.

### Execution record retention

Execution records grow with traffic. Retention policy:

- **Local dev:** records kept until `flux dev --clean`. No auto-cleanup.
- **Self-hosted:** configure retention in `flux.toml`:
  ```toml
  [observability]
  record_retention_days = 30    # delete records older than 30 days
  ```
  A background job in the Data Engine prunes `execution_records`,
  `execution_spans`, `execution_mutations`, and `execution_calls`
  older than the configured threshold. Runs daily.
Errors are retained 3x longer than successful requests by default
(e.g., 90 days vs 30 days) because debugging value concentrates in failures.

---

## 19. Auth

Use middleware. This covers 95% of real apps.

```typescript
// middleware/auth.ts
import { defineMiddleware } from "@flux/functions";

export default defineMiddleware(async (ctx, next) => {
  const token = ctx.headers.get("authorization")?.replace("Bearer ", "");
  if (!token) return ctx.error(401, "UNAUTHORIZED");

  ctx.user = await verifyJWT(token, ctx.secrets.get("JWT_SECRET")!);
  if (!ctx.user) return ctx.error(401, "INVALID_TOKEN");

  return next();
});
```

Assign function groups in `flux.toml`:

```toml
[middleware.groups]
public    = []
protected = ["middleware/auth.ts"]
admin     = ["middleware/auth.ts", "middleware/require_admin.ts"]
```

No policy DSL. No row-level security config. Middleware in JS/TS is simpler,
more flexible, and debuggable with `flux why`.

---

## 20. Build & Deploy

### Build

```bash
flux build [function-name]
```

Pipeline:
1. Scan `functions/` for directories with `index.ts`
2. Bundle TypeScript → single `.js` via esbuild
3. Extract metadata from `defineFunction()` — name, schemas
4. Validate schemas
5. Output to `.flux/build/<name>/`

Artifact:
```
.flux/build/create_user/
  function.js
  metadata.json   { name, entry, git_sha, built_at, input_schema, output_schema }
```

`git_sha` is read from `git rev-parse HEAD` at build time, stored in metadata,
included in every execution record. This enables `flux bug bisect`.

### Deploy

```bash
flux deploy                        # reads target from flux.toml
flux deploy --target local         # hot-swap into running flux dev
flux deploy --target docker        # build Docker image
flux deploy --target k8s           # generate Kubernetes manifests
```

| Target | What happens |
|--------|-------------|
| `local` | Invalidate in-process cache — zero downtime |
| `docker` | Builds `FROM flux/server` image with artifacts baked in |
| `k8s` | Generates deployment manifests referencing the Docker image |

---

## 21. Self-Hosted Deployment

Flux runs entirely on your own infrastructure.

### Docker Compose (simplest)

```yaml
# docker-compose.yml
services:
  postgres:
    image: postgres:16
    environment:
      POSTGRES_DB: flux
      POSTGRES_PASSWORD: ${POSTGRES_PASSWORD}
    volumes:
      - pgdata:/var/lib/postgresql/data

  flux:
    image: flux/server
    ports: ["4000:4000"]
    environment:
      DATABASE_URL: postgres://postgres:${POSTGRES_PASSWORD}@postgres/flux
    depends_on:
      - postgres

volumes:
  pgdata:
```

```bash
docker compose up -d
flux deploy --target docker
```

Two containers: Postgres and Flux. That's the entire production stack.

### Kubernetes

```bash
flux deploy --target k8s    # generates manifests in .flux/k8s/
kubectl apply -f .flux/k8s/
```

Scale horizontally by increasing `replicas`. Every replica runs the full
Flux binary — all modules in-process, stateless against Postgres.

### What you get self-hosted

Everything in the framework:
- One binary running on your infra (all modules in-process)
- Full execution recording and replay
- All CLI commands work (`flux trace`, `flux why`, `flux incident replay`)
- Your Postgres, your data, your network

---

## 22. CLI Reference

✅ = implemented · 🔧 = rewrite in progress (wrong model, not missing) · 📋 = planned

Global flags on every command: `--json` `--no-color` `--quiet` `--verbose`
`--dry-run` `--yes` `--dir <path>`

### Project

| Command | Status | Description |
|---------|--------|-------------|
| `flux init [name]` | 🔧 | Scaffold `flux.toml` + `functions/` + `schemas/` + `tests/` |
| `flux new <name> [--template]` | 🔧 | Full project from template (`blank`, `todo-api`, `ai-backend`, `webhook-worker`) |
| `flux dev [--clean]` | 📋 | Start all services natively (no Docker) + managed local Postgres + hot reload |

### Functions

| Command | Status | Description |
|---------|--------|-------------|
| `flux function create <name>` | ✅ | Scaffold `functions/<name>/index.ts` + `flux.json` |
| `flux function list` | 🔧 | List functions from local API |
| `flux function delete <name>` | 🔧 | Remove from registry + delete directory |
| `flux build [name] [--watch]` | 📋 | Bundle TS → JS via esbuild; write `.flux/build/<name>/` |
| `flux deploy [name] [--target local\|docker\|k8s] [--build]` | 🔧 | Hot-swap local \| build Docker image \| write k8s manifests |
| `flux invoke <name> [--data <json>] [--file]` | 🔧 | Call function via local gateway (`localhost:4000`) |

### Database

| Command | Status | Description |
|---------|--------|-------------|
| `flux db push [--dry-run]` | 📋 | Apply `schemas/*.sql` to local Postgres (diff only, never drops data) |
| `flux db diff` | 📋 | Preview SQL that `push` would run — safe, never executes |
| `flux db migrate [--name]` | 📋 | Save diff as `migrations/<timestamp>_<name>.sql` |
| `flux db seed [--file] [--reset]` | 📋 | Execute `tests/fixtures/*.sql` |
| `flux db reset` | 📋 | Drop + recreate + push + seed |
| `flux db query [--sql] [--file]` | ✅ | Run raw SQL, print as table |
| `flux db shell` | ✅ | Open interactive `psql` session |
| `flux db history <table> [--id]` | ✅ | Before/after mutation history from `state_mutations` |

### Secrets

| Command | Status | Description |
|---------|--------|-------------|
| `flux secrets set <key> <value>` | 🔧 | Write to `.env.local` (auto-loaded by `flux dev`) |
| `flux secrets get <key>` | 🔧 | Read a secret value |
| `flux secrets list` | 🔧 | List keys (values always redacted) |
| `flux secrets delete <key>` | 🔧 | Remove a secret |

### Observability & Debugging

All recording infrastructure exists in Rust (`trace_requests`, `platform_logs`,
`state_mutations` tables). CLI rewrite removes tenant/project auth and points at
`localhost:4000` (the single Flux port).

| Command | Status | Description |
|---------|--------|-------------|
| `flux trace [<id>] [--flame] [--limit] [--function] [--slow]` | 🔧 | List recent traces or render full span tree |
| `flux trace diff <a> <b> [--table]` | 🔧 | Compare two executions field-by-field |
| `flux trace debug <id> [--at] [--interactive]` | 🔧 | Step-through debugger: span-by-span with DB mutations |
| `flux why <id>` | 🔧 | Root cause in 10s: error + mutations + suggested next command |
| `flux debug [<id>] [--replay]` | 🔧 | Interactive debugger — pick from recent errors or deep-dive one |
| `flux fix [<id>]` | 🔧 | Alias for `flux debug` |
| `flux tail [function] [--errors] [--slow] [--auto-debug]` | 🔧 | Live request stream |
| `flux logs [source] [resource] [--follow] [--limit]` | 🔧 | Tail function/service logs |
| `flux errors [--function] [--since]` | 🔧 | Per-function error summary: count, code, p50/p95 |
| `flux state history <table> [--id] [--limit]` | 🔧 | Full row version history |
| `flux state blame <table>` | 🔧 | Last writer per row |
| `flux incident replay <id> [--write] [--live-http]` | 🔧 | Re-run with same input + code SHA; side effects mocked |
| `flux bug bisect --function --good --bad [--threshold]` | 🔧 | Binary-search trace history for first regression commit |
| `flux explain [file]` | ✅ | Dry-run a Data Engine query: compiler output + SQL |

### Queue

| Command | Status | Description |
|---------|--------|-------------|
| `flux queue list [--status] [--function] [--limit]` | 📋 | List jobs (pending/running/failed/dead-letter) |
| `flux queue retry <job-id>` | 📋 | Re-enqueue a failed job |
| `flux queue dead-letter [--limit]` | 📋 | List jobs that exhausted all retries |

### Cron

| Command | Status | Description |
|---------|--------|-------------|
| `flux cron list` | 📋 | List cron jobs: schedule, last/next run, status |
| `flux cron pause <name>` | 📋 | Pause without deleting |
| `flux cron resume <name>` | 📋 | Resume a paused job |
| `flux cron history <name> [--limit]` | 📋 | Recent invocations — each links to a `request-id` |

### Agents

| Command | Status | Description |
|---------|--------|-------------|
| `flux agent create <name>` | 📋 | Scaffold `agents/<name>.ts` with `defineAgent()` template |
| `flux agent list` | 📋 | List agents + deployment status |
| `flux agent deploy <name>` | 📋 | Upload agent definition |
| `flux agent run <name> [--data] [--file]` | 📋 | Run an agent and stream output |
| `flux agent simulate <name> [--data]` | 📋 | Dry-run — show tool calls without executing |

### Events

| Command | Status | Description |
|---------|--------|-------------|
| `flux event list` | 📋 | List registered event types |
| `flux event publish <type> [--data] [--file]` | 📋 | Publish an event manually (for testing) |
| `flux event history <type> [--limit]` | 📋 | Recent events: timestamp, payload, triggered functions |

### Gateway

| Command | Status | Description |
|---------|--------|-------------|
| `flux gateway route list` | 📋 | Show all routes + operational config (auth, rate_limit, cors) |
| `flux gateway route patch <path> [--auth-type] [--rate-limit] [--cors-origins] [--json-schema]` | 📋 | Mutate route config; takes effect immediately via NOTIFY |

### Code Generation

| Command | Status | Description |
|---------|--------|-------------|
| `flux generate [--output] [--watch]` | 📋 | Emit `flux.d.ts` from live DB schema (typed `ctx.db`, `ctx.function.invoke`) |

### Config

| Command | Status | Description |
|---------|--------|-------------|
| `flux config list` | 🔧 | Print effective config from `flux.toml` + `~/.flux/config.json` |
| `flux config get <key>` | 🔧 | Read a single config key |
| `flux config set <key> <value> [--global]` | 🔧 | Write to `flux.toml` or `~/.flux/config.json` |

### Utilities

| Command | Status | Description |
|---------|--------|-------------|
| `flux doctor [<request-id>]` | ✅ | Env health check or per-request diagnosis |
| `flux upgrade [--check] [--version]` | ✅ | Self-update binary via GitHub Releases |

---

## 23. Implementation Phases

### Phase 0 — Prove the debugging magic (2-4 weeks)

Smallest version that validates the core value proposition end-to-end.

**Scope:**
```
flux init     → scaffold project with flux.toml + functions/
flux dev      → starts all services locally (orchestrator + embedded Postgres)
flux invoke   → call a function via gateway
flux trace    → show execution record for that invocation
flux why      → root cause from execution record
```

No agents, no cron, no queue CLI, no middleware, no hot reload.
Just: create project, start runtime, call a function, see the record, debug.

**What this requires building:**
- `server` crate — single binary that composes all 5 modules (Gateway, Runtime,
  API, Data Engine, Queue) into one process on one port (~200 lines)
- `cli/src/dev.rs` — spawn `flux-server` + managed Postgres, combined log output,
  graceful Ctrl+C shutdown, health check (~200 lines)
- `flux.toml` — TOML parser in CLI, `flux init` writes it (~100 lines)
- Local mode — skip tenant resolution, accept all requests (~50 lines)
- Embedded Postgres — auto-start, data directory at `.flux/pgdata/`, port assignment
- Wire `flux trace` and `flux why` CLI commands end-to-end (infrastructure exists
  in `platform_logs` + `state_mutations` tables; the CLI needs to query, format,
  and present the data)

The recording infrastructure exists in Rust. The work is wiring it into a
coherent `flux dev` experience and finishing the CLI output formatting.
Estimated 2-4 weeks.

**What this proves:**
- Execution recording works automatically
- `flux why` genuinely saves debugging time
- Developers want Flux for the debugging alone

---

### Phase 1 — Developer experience (the golden path is fast)

1. **Hot reload** — file watcher + incremental redeploy + cache invalidation
2. **`flux build`** — standalone build step, artifact output to `.flux/build/`
3. **`flux deploy --target local`** — hot-swap without restart
4. **Embedded Postgres improvements** — `flux db push`, `flux db reset`

---

### Phase 2 — Type safety & database

6. **`flux generate`** — TypeScript types from introspect endpoint
7. **`flux db push` + `flux db diff`** — schema management
8. **Error model** — `defineMiddleware()` + error helpers in `@flux/functions`

---

### Phase 3 — Production readiness

9. **`flux test`** — test runner with local fixtures
10. **Middleware system** — `defineMiddleware()` + flux.toml config
11. **`flux worker`** — local queue worker command

---

### Phase 4 — Agents & polish

12. **`flux agent`** — agent runtime, `defineAgent()`, function-as-tools
13. **`flux cron list`** — cron management
14. **Local dashboard** — embedded SPA at `localhost:4000/flux/`
15. **`flux new <template>`** — project templates (auth-api, ai-agent, stripe-payments)
16. **Docker + K8s deploy targets** — `flux deploy --target docker|k8s`

---

## Appendix — Competitive Positioning

| Framework | What you get | Flux advantage |
|---|---|---|
| **Express + Prisma + BullMQ** | DIY stack, no tracing | Integrated framework with automatic execution recording |
| **NestJS** | Structure + DI, no observability | Same structure + full execution history built in |
| **Django / Rails** | Batteries-included, no replay | Same batteries + every request is a record |
| **FastAPI** | Fast Python, manual tracing | Same speed principles + automatic tracing |
| **Temporal** | Workflow engine, high ceremony | Lower friction — functions first, agents when needed |
| **Inngest** | Background jobs | Full execution history across all code, not just jobs |
| **Supabase** | Managed Postgres + Edge Functions | Execution recording, replay debugging, queue, agents |

> **Flux is the Git of backend execution.**
>
> Git made every code change inspectable, diffable, and revertable.
> Flux makes every backend execution inspectable, diffable, and replayable.

---

## Appendix — Versioning

Flux ships three versioned artifacts:

| Artifact | Registry | Example |
|----------|----------|---------|
| `flux` CLI binary | GitHub Releases / Homebrew | `flux@0.3.0` |
| `@flux/functions` SDK | npm | `@flux/functions@0.3.0` |
| `@flux/testing` SDK | npm | `@flux/testing@0.3.0` |

**Compatibility contract:** The CLI and SDK versions are released in lockstep.
A given `@flux/functions@0.x` works with `flux@0.x`. The Runtime validates
the SDK version at execution time and rejects mismatches with a clear error:
`"@flux/functions@0.2.0 requires flux runtime >=0.2.0, got 0.1.3"`.

During `0.x` (pre-1.0), breaking changes are allowed between minor versions.
After `1.0`, semver is enforced: minor versions are backwards-compatible,
major versions may break.

---

*For implementation details and code reuse paths, see the source at `github.com/flux-framework/flux`.*
