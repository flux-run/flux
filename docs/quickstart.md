# Quickstart — Build a debuggable backend in 5 minutes

Flux gives you functions, an integrated database, secrets, queues, and automatic
execution recording — all from a single CLI. No Docker, no `.env`
files, no infrastructure setup.

---

## Prerequisites

- Node.js 18+
- The Flux CLI:

```bash
# macOS
brew install flux

# or from npm
npm install -g @flux/cli

# or curl
curl -fsSL https://flux.dev/install.sh | sh
```

---

## Step 1 — Create a project

```bash
flux init my-app && cd my-app
```

This creates:
```
my-app/
├── flux.toml               # project config
├── functions/
│   └── hello/
│       └── index.ts         # starter function
├── schemas/                 # SQL schema files (empty)
└── tests/                   # test directory (empty)
```

---

## Step 2 — Start the dev server

```bash
flux dev
```

This starts all 5 services + a local Postgres instance. No Docker required.

```
✓ Postgres    → :5432  (data at .flux/pgdata/)
✓ API         → :8080
✓ Data Engine → :8082
✓ Runtime     → :8083
✓ Queue       → :8084
✓ Gateway     → :4000

Flux running at http://localhost:4000
Watching functions/ for changes...
```

---

## Step 3 — Write a function

Edit `functions/hello/index.ts`:

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";

export default defineFunction({
  name: "hello",
  input:  z.object({ name: z.string() }),
  output: z.object({ message: z.string() }),
  handler: async ({ input, ctx }) => {
    ctx.log.info(`Greeting ${input.name}`);
    return { message: `Hello, ${input.name}!` };
  },
});
```

Save the file. Hot reload picks it up in <200ms.

---

## Step 4 — Call it

```bash
flux invoke hello --data '{"name": "World"}'
```

```json
{ "message": "Hello, World!" }
```

Or via HTTP:

```bash
curl -X POST http://localhost:4000/hello \
  -H "Content-Type: application/json" \
  -d '{"name": "World"}'
```

Every function directory in `functions/` becomes a `POST` endpoint automatically.

---

## Step 5 — See the execution record

Every request gets a `x-request-id`. Trace it:

```bash
flux trace <request-id>
```

```
Trace a3f9d2b1-...  12ms end-to-end

  09:41:02.000  +0ms   ▶ [gateway/hello]   route matched: POST /hello
  09:41:02.002  +2ms   · [runtime/hello]   bundle cache hit
  09:41:02.004  +2ms   ▶ [runtime/hello]   executing function
  09:41:02.010  +6ms   · [runtime/hello]   Greeting World
  09:41:02.012  +2ms   ■ [runtime/hello]   execution completed (8ms)

  5 spans  •  12ms total
```

---

## Step 6 — Add a database table

Create `schemas/users.sql`:

```sql
CREATE TABLE IF NOT EXISTS users (
  id         UUID PRIMARY KEY DEFAULT gen_random_uuid(),
  name       TEXT NOT NULL,
  email      TEXT NOT NULL UNIQUE,
  created_at TIMESTAMP DEFAULT NOW()
);
```

Push it:

```bash
flux db push
```

---

## Step 7 — Write a function that uses the database

Create `functions/create_user/index.ts`:

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";

export default defineFunction({
  name: "create_user",
  input:  z.object({ name: z.string(), email: z.string().email() }),
  output: z.object({ id: z.string() }),
  handler: async ({ input, ctx }) => {
    const user = await ctx.db.users.insert(input);
    ctx.log.info(`Created user ${user.id}`);
    return { id: user.id };
  },
});
```

Call it:

```bash
flux invoke create_user --data '{"name": "Ada", "email": "ada@example.com"}'
```

Now trace it — you'll see the database mutation in the execution record:

```bash
flux trace <request-id>
```

```
Trace 7f3a1b2c-...  18ms end-to-end

  09:42:01.000  +0ms   ▶ [gateway/create_user]   route matched
  09:42:01.003  +3ms   ▶ [runtime/create_user]   executing function
  09:42:01.008  +5ms   · [db/users]               INSERT 1 row (5ms)
  09:42:01.015  +7ms   ■ [runtime/create_user]   completed (12ms)

  State changes:
    users  INSERT  id=e4a9c3f1  name="Ada"  email="ada@example.com"
```

---

## Step 8 — Debug with `flux why`

If something fails, run:

```bash
flux why <request-id>
```

```
✗  POST /create_user  (24ms, 500)

ROOT CAUSE:
  error: duplicate key value violates unique constraint "users_email_key"
  span:  db/users INSERT (line 8 of create_user/index.ts)

STATE AT FAILURE:
  users  id=e4a9c3f1  email="ada@example.com"  (already exists)

FIX SUGGESTION:
  Check for existing user before insert, or use ON CONFLICT
```

One command. Root cause, state context, fix suggestion.

---

## Next steps

- [Core Concepts](concepts.md) — understand execution records, the ctx object, and flux.toml
- [Framework](framework.md) — the complete spec (architecture, API, config, phases)
- [Observability](observability.md) — N+1 detection, slow spans, `flux trace diff`
- [Examples](examples/todo-api.md) — full CRUD API with database + tracing
