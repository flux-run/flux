# Core Concepts

Flux has a small number of concepts. This page covers all of them.
For the complete spec, see [framework.md](framework.md).

---

## What Flux Is

Flux is a standalone open-source backend framework. There is no managed cloud —
you run it locally, in Docker, or on Kubernetes.

```
flux init      → create project
flux dev       → local dev server
flux deploy    → push to any target (local, docker, k8s)
flux test      → test runner
flux trace     → execution records
flux why       → root cause
```

---

## Execution Record

The core primitive. Every function call automatically captures:

- **Input/output** — what went in, what came out
- **Spans** — timing for every layer (gateway, runtime, DB, external calls)
- **Database mutations** — before/after JSONB for every INSERT, UPDATE, DELETE
- **External calls** — HTTP requests, tool calls, queue pushes
- **Code SHA** — which git commit was deployed

All linked by a single `request_id`. This is what makes `flux trace`,
`flux why`, `flux incident replay`, and `flux bug bisect` possible.

```typescript
interface ExecutionRecord {
  request_id:    string;
  function_name: string;
  code_sha:      string;
  input:         JsonValue;
  output:        JsonValue | null;
  error:         FluxError | null;
  duration_ms:   number;
  spans:         ExecutionSpan[];
  db_mutations:  DbMutation[];
  external_calls: ExternalCall[];
}
```

---

## Functions

Every function lives in its own directory under `functions/`:

```
functions/
├── hello/
│   └── index.ts
├── create_user/
│   └── index.ts
└── send_email/
    └── index.ts
```

Every function directory becomes a `POST` endpoint:
- `functions/hello/` → `POST /hello`
- `functions/create_user/` → `POST /create_user`

Functions are defined with `defineFunction()` from `@flux/functions`:

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";

export default defineFunction({
  name: "create_user",
  input:  z.object({ name: z.string(), email: z.string().email() }),
  output: z.object({ id: z.string() }),
  handler: async ({ input, ctx }) => {
    const user = await ctx.db.users.insert(input);
    return { id: user.id };
  },
});
```

No raw handlers, no manual routing, no decorators.

---

## The `ctx` Object

Every handler receives `ctx` — the single interface to all Flux capabilities:

| Property | What it does |
|---|---|
| `ctx.db` | Database access — typed from `schemas/` via `flux generate` |
| `ctx.queue` | Push async jobs — `ctx.queue.push("send_email", payload)` |
| `ctx.workflow` | Start workflows — `ctx.workflow.start("onboarding", input)` |
| `ctx.function` | Call other functions — `ctx.function.invoke("validate", data)` |
| `ctx.secrets` | Read secrets — `ctx.secrets.get("STRIPE_KEY")` |
| `ctx.tools` | Third-party integrations (Stripe, OpenAI, etc.) |
| `ctx.log` | Structured logging — attached to execution record |
| `ctx.error()` | Throw structured error — stops execution |
| `ctx.requestId` | UUID propagated through entire execution |
| `ctx.headers` | Request headers |
| `ctx.user` | Set by auth middleware |

No imports, no client instantiation, no connection strings.

---

## Database

Flux manages your application database. Postgres only. SQL schemas are the
source of truth — no ORM.

### Schema files

```sql
-- schemas/users.sql
CREATE TABLE IF NOT EXISTS users (
  id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name       TEXT NOT NULL,
  email      TEXT NOT NULL UNIQUE,
  created_at TIMESTAMP DEFAULT NOW()
);
```

### Commands

```bash
flux db push       # apply schemas/*.sql
flux db diff       # preview changes (never executes)
flux db migrate    # save diff as timestamped migration
flux db seed       # apply tests/fixtures/*.sql
flux db reset      # drop + recreate + push + seed
```

### ctx.db

`ctx.db` is a thin typed wrapper that compiles to SQL inside the Data Engine.
It is **not** an ORM — schemas are raw SQL, types are derived by `flux generate`
from `information_schema`.

```typescript
// Typed accessors (compiled to SQL by Data Engine)
const user = await ctx.db.users.insert({ name: "Ada", email: "ada@acme.com" });
const users = await ctx.db.users.findMany({ where: { email: { eq: "ada@acme.com" } } });

// Raw SQL escape hatch (also goes through Data Engine, also recorded)
const results = await ctx.db.query("SELECT * FROM users WHERE created_at > $1", [date]);
```

Both paths go through the Data Engine, so both are recorded in execution records.
Every write captures before/after state — that's the foundation of `flux why`.

---

## flux.toml

One config file per project:

```toml
[project]
name = "my-app"
version = "0.1.0"

[dev]
port = 4000
hot_reload = true

[deploy]
target = "local"   # "local" | "docker" | "k8s"

[limits]
timeout_ms = 30000
memory_mb = 128

[observability]
record_sample_rate = 1.0   # every execution is a record

[middleware]
global = ["middleware/auth.ts"]
```

---

## Secrets

```bash
flux secrets set STRIPE_KEY sk_live_...
flux secrets list
flux secrets delete STRIPE_KEY
```

Inside a function: `ctx.secrets.get("STRIPE_KEY")`.
Locally stored in `.env.local` (gitignored). Never committed to version control.

---

## Queue

Push async jobs from any function:

```typescript
await ctx.queue.push("send_email", { user_id: user.id }, { delay: "5m" });
```

Jobs execute via the Queue service → Runtime. Same execution recording, same
`flux trace` / `flux why` debugging. See [queue.md](queue.md) for internals.

---

## Workflows

Multi-step, long-running processes:

```typescript
import { defineWorkflow } from "@flux/functions";

export default defineWorkflow({
  name: "onboarding",
  trigger: { type: "function", function: "create_user" },
  steps: [
    {
      name: "send_welcome_email",
      function: "send_email",
      input: (ctx) => ({ to: ctx.trigger.output.email, subject: "Welcome!" }),
    },
    {
      name: "assign_trial_plan",
      function: "assign_plan",
      input: (ctx) => ({ user_id: ctx.trigger.output.id }),
    },
  ],
});
```

Each step is a function call. Each step produces an execution record.
If a step fails, the workflow pauses and can be resumed.

---

## Middleware

Request middleware runs before the handler:

```typescript
import { defineMiddleware } from "@flux/functions";

export default defineMiddleware(async (ctx, next) => {
  const token = ctx.headers.get("authorization")?.replace("Bearer ", "");
  if (!token) return ctx.error(401, "UNAUTHORIZED");
  ctx.user = await verifyToken(token);
  return next();
});
```

Middleware is assigned in `flux.toml` or per-function in `flux.json`.

---

## Architecture

`flux dev` starts the full stack via Docker Compose (`flux stack up`):

| Service | Port | Responsibility |
|---|---|---|
| Gateway | `:8081` | Routing, auth, rate limiting, trace roots |
| Runtime | `:8083` | Deno V8 execution, secrets, tool dispatch |
| API | `:8080` | Function registry, logs, schema management |
| Data Engine | `:8082` | DB queries, mutation recording, hooks, cron |
| Queue | `:8084` | Async jobs, retries, dead letter |

All services are Rust + Axum. The Runtime uses `deno_core` for V8 isolate
execution. Database is Postgres.

Only the Gateway is exposed to the internet. All other services communicate
internally via `x-service-token`.

---

*For the complete specification, see [framework.md](framework.md).*
