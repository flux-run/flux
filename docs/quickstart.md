# Quickstart — Build a backend in 5 minutes

Fluxbase gives you serverless functions, a managed database, built-in secrets,
and end-to-end distributed tracing — all driven from a single CLI.

---

## Prerequisites

- Node.js 18+
- A Fluxbase account at [fluxbase.co](https://fluxbase.co)
- The Fluxbase CLI installed and authenticated:

```bash
npm install -g @fluxbase/cli
flux auth login
```

---

## Step 1 — Create a project

```bash
mkdir my-backend && cd my-backend
flux init
```

This creates a `flux.json` manifest and links the directory to a new project in
your tenant.

---

## Step 2 — Write your first function

Create `index.ts`:

```typescript
import { defineFunction } from "@fluxbase/functions";
import { z } from "zod";

export default defineFunction({
  name: "greet",
  input:  z.object({ name: z.string() }),
  output: z.object({ message: z.string() }),
  handler: async ({ input, ctx }) => {
    ctx.log(`Greeting ${input.name}`);
    return { message: `Hello, ${input.name}!` };
  },
});
```

> Raw functions (without `defineFunction`) are also supported:
>
> ```javascript
> export default async function(ctx) {
>   return { message: `Hello, ${ctx.payload.name}!` };
> }
> ```

---

## Step 3 — Deploy

```bash
flux deploy
```

The CLI bundles your function, uploads it to the control plane, and returns the
function ID.

---

## Step 4 — Invoke

```bash
flux invoke greet --data '{"name": "World"}'
```

Expected output:

```json
{ "message": "Hello, World!" }
```

Or call it directly via the gateway:

```bash
curl -X POST https://YOUR_GATEWAY/greet \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer YOUR_API_KEY" \
  -d '{"name": "World"}'
```

---

## Step 5 — Observe

Every gateway request is assigned a `x-request-id`. Trace it end-to-end:

```bash
flux trace <request-id>
```

Sample output:

```
Trace a3f9d2b1-...  142ms end-to-end

  12:00:01.000  +0ms    ▶ [gateway/greet]   INFO   route matched: POST /greet
  12:00:01.008  +8ms    · [runtime/greet]   INFO   bundle cache hit
  12:00:01.012  +4ms    ▶ [runtime/greet]   INFO   executing function
  12:00:01.017  +5ms    ■ [runtime/greet]   INFO   execution completed (5ms)
```

---

## Next steps

- [Core Concepts](concepts.md) — understand the runtime model
- [Observability](observability.md) — slow spans, N+1 detection, index hints
- [CLI Reference](cli.md) — full command list
- [Examples](examples/) — Todo API, Webhook Worker, AI Backend
