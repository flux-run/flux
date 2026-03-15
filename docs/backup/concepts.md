# Core Concepts

---

## Functions

A **function** is the primary compute unit in Flux.  Functions are
language-independent compute units: write them in JavaScript, TypeScript, Rust,
Go, or any language that compiles to WebAssembly.  Each function is deployed as
an isolated bundle and executed inside one of two sandboxed runtimes:

| Runtime | `flux.json` field | Languages | Use cases |
|---|---|---|---|
| **Deno V8** | `"runtime": "deno"` | JavaScript, TypeScript | APIs, scripts, workflows, AI agents |
| **WASM** | `"runtime": "wasm"` | Rust, Go, C, AssemblyScript, … | CPU-bound compute, ML inference, native libraries |

See [WASM Runtime](./wasm-runtime.md) for the full design and language guides.

### Handler signatures

**Raw handler** (quick scripts, prototyping):

```javascript
// index.js
export default async function(ctx) {
  const { name } = ctx.payload;
  ctx.log(`called with name=${name}`);
  return { message: `Hello, ${name}!` };
}
```

**Schema-validated handler** (production, type-safe):

```typescript
// index.ts
import { defineFunction } from "@flux/functions";
import { z } from "zod";

export default defineFunction({
  name: "create_todo",
  input:  z.object({ title: z.string(), done: z.boolean().default(false) }),
  output: z.object({ id: z.string() }),
  handler: async ({ input }) => {
    // input is fully typed as { title: string; done: boolean }
    const todo = await db.todos.insert(input);
    return { id: todo.id };
  },
});
```

### The `ctx` object

| Property | Type | Description |
|---|---|---|
| `ctx.payload` | `unknown` | Raw request body (JSON-decoded) |
| `ctx.secrets.get(key)` | `string \| null` | Named secret value |
| `ctx.env` | `Record<string, string>` | Same secrets as a flat map |
| `ctx.log(msg, level?)` | `void` | Emit a structured log line |

`defineFunction` exposes `input` (validated payload) via `{ input, ctx }`.

### Manifest — `flux.json`

**Deno (JavaScript / TypeScript):**

```json
{
  "runtime": "deno",
  "entry": "index.ts"
}
```

**WASM (Rust, Go, C, AssemblyScript, …):**

```json
{
  "runtime": "wasm",
  "entry":   "handler.wasm",
  "build":   "cargo build --target wasm32-wasip1 --release && cp target/wasm32-wasip1/release/my_fn.wasm handler.wasm",
  "memory_mb": 64
}
```

| Field | Description |
|---|---|
| `runtime` | `"deno"` or `"wasm"` |
| `entry` | Entry file — `.ts`/`.js` for Deno, `.wasm` for WASM |
| `build` | Optional shell command run by `flux deploy` before upload |
| `memory_mb` | WASM only — linear memory cap (default 64 MB) |

---

## Database

Flux provides a fully managed Postgres database accessed through a
structured query API.  You never write SQL directly.

### Query format

Queries are JSON objects submitted to the gateway's `/db/query` endpoint:

```json
{
  "table":     "todos",
  "operation": "select",
  "columns":   ["id", "title", "done"],
  "filters":   [{ "column": "done", "op": "eq", "value": false }],
  "limit":     20,
  "offset":    0
}
```

**Supported operations:** `select`, `insert`, `update`, `delete`

**Supported filter operators:** `eq`, `neq`, `gt`, `gte`, `lt`, `lte`,
`like`, `ilike`, `is_null`, `not_null`

### Typed SDK client

Use the generated SDK for a type-safe experience:

```typescript
import { createClient } from "@flux/sdk";

const flux = createClient({
  url:       ctx.env.GATEWAY_URL,
  apiKey:    ctx.env.API_KEY,
  projectId: ctx.env.PROJECT_ID,
});

// Fully typed — IDE infers the row shape
const todos = await flux.db.todos
  .where("done", "eq", false)
  .orderBy("created_at", "desc")
  .limit(20)
  .execute();
```

Generate the typed SDK for your schema:

```bash
flux sdk generate
```

This writes `flux.d.ts` to your project directory.

### Edge query cache

Read-only `select` queries are automatically cached at the gateway for 30 s.
Writes invalidate the cache for the affected table immediately.

Cache status is exposed via the `x-cache` response header (`HIT` / `MISS` /
`BYPASS`) and as a span in distributed traces.

---

## Secrets

Secrets are key-value pairs stored encrypted at rest and injected into function
context at invocation time.  They are scoped per project.

```bash
# Set a secret
flux secrets set OPENAI_API_KEY sk-...

# List secrets (values redacted)
flux secrets list

# Delete a secret
flux secrets delete OPENAI_API_KEY
```

Inside a function:

```javascript
const apiKey = ctx.secrets.get("OPENAI_API_KEY");
// or
const apiKey = ctx.env.OPENAI_API_KEY;
```

---

## Gateway

The **gateway** is the public-facing HTTP server for your tenant.  It:

- Routes requests to the correct function by path (`/FUNCTION_NAME`)
- Handles CORS, authentication, and rate limiting
- Proxies structure queries to the data engine (with edge-layer caching)
- Assigns a `x-request-id` to every inbound request for distributed tracing

---

## Deployments

A **deployment** is an immutable snapshot of a function + its bundle.

```bash
flux deploy            # deploy current directory
flux deployments list  # show deployment history
```

Each deploy produces a versioned artifact.  Traffic switches to the new
revision immediately after a successful deploy.

---

## Projects and tenants

| Concept | Scope | Description |
|---|---|---|
| **Tenant** | Account-level | Billing and user boundary |
| **Project** | Tenant-level | Isolated namespace (DB, secrets, functions) |

One tenant can have many projects (e.g. `staging`, `production`).
