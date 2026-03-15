# Example — Todo API

A classic CRUD API backed by Flux's managed database.  Four functions,
one schema, zero infrastructure to manage.

---

## What you'll build

| Endpoint | Function | Description |
|---|---|---|
| `POST /create_todo` | `create_todo` | Create a new to-do item |
| `POST /list_todos` | `list_todos` | List all to-dos, with filtering |
| `POST /update_todo` | `update_todo` | Mark a to-do done (or update title) |
| `POST /delete_todo` | `delete_todo` | Delete a to-do by ID |

---

## Step 1 — Define the schema

In the [Flux dashboard](https://dashboard.fluxbase.co), create a table
`todos` with the following columns:

| Column | Type | Notes |
|---|---|---|
| `id` | `uuid` | Auto-generated primary key |
| `title` | `text` | Required |
| `done` | `boolean` | Default `false` |
| `created_at` | `timestamptz` | Auto-set to `now()` |
| `updated_at` | `timestamptz` | Auto-updated on write |

---

## Step 2 — Create the functions

```bash
mkdir todo-api && cd todo-api
flux init
```

Create `create_todo/index.ts`:

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";

export default defineFunction({
  name: "create_todo",
  input:  z.object({ title: z.string().min(1).max(255) }),
  output: z.object({ id: z.string(), title: z.string(), done: z.boolean() }),

  handler: async ({ input, ctx }) => {
    const flux = createClient({
      url:       ctx.env.GATEWAY_URL,
      apiKey:    ctx.env.API_KEY,
      projectId: ctx.env.PROJECT_ID,
    });

    const [todo] = await flux.db.todos
      .insert({ title: input.title, done: false })
      .returning(["id", "title", "done"])
      .execute();

    ctx.log(`Created todo: ${todo.id}`);
    return todo;
  },
});
```

Create `list_todos/index.ts`:

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";

export default defineFunction({
  name: "list_todos",
  input: z.object({
    done:   z.boolean().optional(),
    limit:  z.number().int().min(1).max(100).default(20),
    offset: z.number().int().min(0).default(0),
  }),

  handler: async ({ input, ctx }) => {
    const flux = createClient({
      url:       ctx.env.GATEWAY_URL,
      apiKey:    ctx.env.API_KEY,
      projectId: ctx.env.PROJECT_ID,
    });

    let query = flux.db.todos
      .select({ id: true, title: true, done: true, created_at: true })
      .orderBy("created_at", "desc")
      .limit(input.limit)
      .offset(input.offset);

    if (input.done !== undefined) {
      query = query.where("done", "eq", input.done);
    }

    return { todos: await query.execute() };
  },
});
```

Create `update_todo/index.ts`:

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";

export default defineFunction({
  name: "update_todo",
  input: z.object({
    id:    z.string().uuid(),
    done:  z.boolean().optional(),
    title: z.string().min(1).optional(),
  }),

  handler: async ({ input, ctx }) => {
    const flux = createClient({
      url:       ctx.env.GATEWAY_URL,
      apiKey:    ctx.env.API_KEY,
      projectId: ctx.env.PROJECT_ID,
    });

    const { id, ...updates } = input;
    if (Object.keys(updates).length === 0) {
      throw new Error("Provide at least one field to update");
    }

    const [todo] = await flux.db.todos
      .update(updates)
      .where("id", "eq", id)
      .returning(["id", "title", "done"])
      .execute();

    if (!todo) throw new Error(`Todo ${id} not found`);
    return todo;
  },
});
```

Create `delete_todo/index.ts`:

```typescript
import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";

export default defineFunction({
  name: "delete_todo",
  input:  z.object({ id: z.string().uuid() }),
  output: z.object({ deleted: z.boolean() }),

  handler: async ({ input, ctx }) => {
    const flux = createClient({
      url:       ctx.env.GATEWAY_URL,
      apiKey:    ctx.env.API_KEY,
      projectId: ctx.env.PROJECT_ID,
    });

    await flux.db.todos
      .delete()
      .where("id", "eq", input.id)
      .execute();

    return { deleted: true };
  },
});
```

---

## Step 3 — Set secrets

```bash
flux secrets set GATEWAY_URL  "https://YOUR_GATEWAY_URL"
flux secrets set API_KEY      "YOUR_API_KEY"
flux secrets set PROJECT_ID   "YOUR_PROJECT_ID"
```

---

## Step 4 — Deploy

```bash
flux deploy create_todo
flux deploy list_todos
flux deploy update_todo
flux deploy delete_todo
```

---

## Step 5 — Try it

```bash
# Create
flux invoke create_todo --data '{"title": "Buy groceries"}'
# → { "id": "abc...", "title": "Buy groceries", "done": false }

# List open todos
flux invoke list_todos --data '{"done": false}'

# Complete one
flux invoke update_todo --data '{"id": "abc...", "done": true}'

# Delete
flux invoke delete_todo --data '{"id": "abc..."}'
```

---

## Tracing a request

```bash
# The gateway returns x-request-id on every call
curl -D - https://YOUR_GATEWAY/list_todos -d '{"done":false}' | grep x-request-id

flux trace <that-id>
```

If `list_todos` makes more than 3 queries to `todos` in a single request (e.g.
from a loop), the trace will flag it as an N+1 pattern automatically.
