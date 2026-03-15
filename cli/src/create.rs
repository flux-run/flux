//! `flux create <name> [--template <template>]`
//!
//! Scaffold a full Flux project from an official template so a developer
//! can go from zero to a deployed backend in under five minutes.
//!
//! Usage:
//!   flux create my-app                    # interactive template picker
//!   flux create my-app --template todo-api
//!   flux create my-app --template webhook-worker
//!   flux create my-app --template ai-backend

use std::{
    fs,
    io::{self, BufRead, Write},
    path::Path,
};

use colored::Colorize;

// ── Template registry ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy)]
pub enum Template {
    TodoApi,
    WebhookWorker,
    AiBackend,
}

impl Template {
    const ALL: &'static [(&'static str, &'static str, Template)] = &[
        ("todo-api",       "CRUD API backed by the managed database",              Template::TodoApi),
        ("webhook-worker", "Secure webhook receiver with signature verification",  Template::WebhookWorker),
        ("ai-backend",     "AI text classification with caching via OpenAI",       Template::AiBackend),
    ];

    pub fn from_str(s: &str) -> Option<Template> {
        Self::ALL.iter().find(|(k, _, _)| *k == s).map(|(_, _, t)| *t)
    }

    pub fn slug(&self) -> &'static str {
        match self {
            Template::TodoApi       => "todo-api",
            Template::WebhookWorker => "webhook-worker",
            Template::AiBackend     => "ai-backend",
        }
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

pub async fn execute(name: String, template: Option<String>) -> anyhow::Result<()> {
    println!();
    println!("{}", "flux create".bold().cyan());
    println!();

    // Resolve template (flag → interactive picker)
    let tpl = if let Some(s) = template {
        Template::from_str(&s).ok_or_else(|| {
            let valid: Vec<&str> = Template::ALL.iter().map(|(k, _, _)| *k).collect();
            anyhow::anyhow!(
                "Unknown template '{}'. Valid options: {}",
                s,
                valid.join(", ")
            )
        })?
    } else {
        pick_template_interactive()?
    };

    // Ensure the target directory doesn't already exist
    let root = Path::new(&name);
    if root.exists() {
        anyhow::bail!(
            "Directory '{}' already exists. Choose a different project name.",
            name
        );
    }

    println!();
    println!("  Scaffolding {} template into {}/ ...", tpl.slug().cyan().bold(), name.bold());
    println!();

    match tpl {
        Template::TodoApi       => scaffold_todo_api(root, &name)?,
        Template::WebhookWorker => scaffold_webhook_worker(root, &name)?,
        Template::AiBackend     => scaffold_ai_backend(root, &name)?,
    }

    // Every template gets a flux.toml so `flux dev` works immediately
    // without needing a separate `flux init`.
    write_file(&root.join("flux.toml"), &flux_toml(&name))?;

    // Summary
    println!("  {} Project created: {}/", "✔".green().bold(), name.bold());
    println!();
    println!("  {}", "Next steps:".bold());
    println!("    {}", format!("cd {}", name).cyan().bold());
    println!("    {}", "flux dev          # start local server + watch".cyan().bold());
    println!("    {}", "# then in another terminal:".dimmed());
    println!("    {}", "flux login        # create admin account".dimmed());
    println!("    {}", format!("flux invoke {} --payload '{{...}}'", first_function(tpl)).dimmed());
    println!();
    println!("  See {} for the full setup guide.", "README.md".bold());

    Ok(())
}

// ── Interactive template picker ───────────────────────────────────────────────

fn pick_template_interactive() -> anyhow::Result<Template> {
    println!("  {} Select a template:", "?".cyan().bold());
    println!();
    for (i, (key, desc, _)) in Template::ALL.iter().enumerate() {
        println!("    {}  {}  {}", (i + 1).to_string().bold(), key.cyan(), desc.dimmed());
    }
    println!();
    print!("  Enter number [1]: ");
    io::stdout().flush()?;

    let stdin = io::stdin();
    let line = stdin.lock().lines().next().unwrap_or(Ok(String::new()))?;
    let choice: usize = line.trim().parse().unwrap_or(1);

    let idx = choice.saturating_sub(1).min(Template::ALL.len() - 1);
    Ok(Template::ALL[idx].2)
}

fn first_function(tpl: Template) -> &'static str {
    match tpl {
        Template::TodoApi       => "create_todo",
        Template::WebhookWorker => "on_webhook",
        Template::AiBackend     => "classify_text",
    }
}

// ── File creation helper ──────────────────────────────────────────────────────

fn write_file(path: &Path, content: &str) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    println!("    {} {}", "+".green(), path.display().to_string().dimmed());
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Template: todo-api
// ─────────────────────────────────────────────────────────────────────────────

fn scaffold_todo_api(root: &Path, name: &str) -> anyhow::Result<()> {
    // create_todo
    write_file(&root.join("functions/create_todo/index.ts"), CREATE_TODO_TS)?;
    write_file(&root.join("functions/create_todo/flux.json"), FLUX_JSON)?;
    write_file(&root.join("functions/create_todo/package.json"), &fn_package_json("create_todo"))?;

    // list_todos
    write_file(&root.join("functions/list_todos/index.ts"), LIST_TODOS_TS)?;
    write_file(&root.join("functions/list_todos/flux.json"), FLUX_JSON)?;
    write_file(&root.join("functions/list_todos/package.json"), &fn_package_json("list_todos"))?;

    // update_todo
    write_file(&root.join("functions/update_todo/index.ts"), UPDATE_TODO_TS)?;
    write_file(&root.join("functions/update_todo/flux.json"), FLUX_JSON)?;
    write_file(&root.join("functions/update_todo/package.json"), &fn_package_json("update_todo"))?;

    // delete_todo
    write_file(&root.join("functions/delete_todo/index.ts"), DELETE_TODO_TS)?;
    write_file(&root.join("functions/delete_todo/flux.json"), FLUX_JSON)?;
    write_file(&root.join("functions/delete_todo/package.json"), &fn_package_json("delete_todo"))?;

    // TypeScript schema (schemas/ folder — consistent with flux init)
    write_file(&root.join("schemas/todos.schema.ts"), TODO_SCHEMA_TS)?;

    // README
    write_file(&root.join("README.md"), &todo_readme(name))?;
    write_file(&root.join(".env.example"), TODO_ENV_EXAMPLE)?;
    write_file(&root.join(".gitignore"), GITIGNORE)?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Template: webhook-worker
// ─────────────────────────────────────────────────────────────────────────────

fn scaffold_webhook_worker(root: &Path, name: &str) -> anyhow::Result<()> {
    write_file(&root.join("functions/on_webhook/index.ts"), WEBHOOK_TS)?;
    write_file(&root.join("functions/on_webhook/flux.json"), FLUX_JSON)?;
    write_file(&root.join("functions/on_webhook/package.json"), &fn_package_json("on_webhook"))?;

    write_file(&root.join("schemas/webhook_events.schema.ts"), WEBHOOK_SCHEMA_TS)?;
    write_file(&root.join("README.md"), &webhook_readme(name))?;
    write_file(&root.join(".env.example"), WEBHOOK_ENV_EXAMPLE)?;
    write_file(&root.join(".gitignore"), GITIGNORE)?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Template: ai-backend
// ─────────────────────────────────────────────────────────────────────────────

fn scaffold_ai_backend(root: &Path, name: &str) -> anyhow::Result<()> {
    write_file(&root.join("functions/classify_text/index.ts"), AI_TS)?;
    write_file(&root.join("functions/classify_text/flux.json"), FLUX_JSON)?;
    write_file(&root.join("functions/classify_text/package.json"), &fn_package_json("classify_text"))?;

    write_file(&root.join("schemas/classifications.schema.ts"), AI_SCHEMA_TS)?;
    write_file(&root.join("README.md"), &ai_readme(name))?;
    write_file(&root.join(".env.example"), AI_ENV_EXAMPLE)?;
    write_file(&root.join(".gitignore"), GITIGNORE)?;

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared helpers
// ─────────────────────────────────────────────────────────────────────────────

fn fn_package_json(name: &str) -> String {
    format!(
        r#"{{
  "name": "{name}",
  "version": "1.0.0",
  "type": "module",
  "dependencies": {{
    "@flux/functions": "^0.1.0",
    "zod": "^3.22.0"
  }},
  "devDependencies": {{
    "esbuild": "^0.20.0",
    "typescript": "^5.0.0"
  }},
  "scripts": {{
    "build": "esbuild index.ts --bundle --format=iife --global-name=__flux_fn --outfile=dist/index.js"
  }}
}}
"#,
        name = name
    )
}

fn flux_toml(name: &str) -> String {
    format!(
        r#"[project]
name    = "{name}"
version = "1"

[dev]
port            = 4000
hot_reload      = true
reload_debounce = 150    # ms

[database]
# Leave blank to use embedded Postgres (zero config).
# Set DATABASE_URL in .env to connect to your own Postgres instance.
url = ""

[limits]
timeout_ms = 30_000
memory_mb  = 128

[observability]
sampling_rate  = 1.0
slow_span_ms   = 500
retention_days = 90

[auth]
jwt_algorithm = "HS256"
token_expiry  = "24h"
"#,
        name = name
    )
}

// ── Shared file constants ─────────────────────────────────────────────────────

const FLUX_JSON: &str = r#"{
  "runtime": "deno",
  "entry": "index.ts"
}
"#;

const GITIGNORE: &str = r#"node_modules/
dist/
*.js.map
.env
.flux/dev/
"#;

// ─────────────────────────────────────────────────────────────────────────────
// todo-api — source files
// ─────────────────────────────────────────────────────────────────────────────

const CREATE_TODO_TS: &str = r#"import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";

export default defineFunction({
  name: "create_todo",
  description: "Create a new to-do item",
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
"#;

const LIST_TODOS_TS: &str = r#"import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";

export default defineFunction({
  name: "list_todos",
  description: "List to-do items with optional filtering",
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

    const todos = await query.execute();
    return { todos, count: todos.length };
  },
});
"#;

const UPDATE_TODO_TS: &str = r#"import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";

export default defineFunction({
  name: "update_todo",
  description: "Update title or completion status of a to-do",
  input: z.object({
    id:    z.string().uuid(),
    done:  z.boolean().optional(),
    title: z.string().min(1).optional(),
  }).refine(d => d.done !== undefined || d.title !== undefined, {
    message: "Provide at least one field to update",
  }),

  handler: async ({ input, ctx }) => {
    const flux = createClient({
      url:       ctx.env.GATEWAY_URL,
      apiKey:    ctx.env.API_KEY,
      projectId: ctx.env.PROJECT_ID,
    });

    const { id, ...updates } = input;
    const [todo] = await flux.db.todos
      .update(updates)
      .where("id", "eq", id)
      .returning(["id", "title", "done"])
      .execute();

    if (!todo) throw new Error(`Todo ${id} not found`);
    ctx.log(`Updated todo: ${id}`);
    return todo;
  },
});
"#;

const DELETE_TODO_TS: &str = r#"import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";

export default defineFunction({
  name: "delete_todo",
  description: "Permanently delete a to-do item",
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

    ctx.log(`Deleted todo: ${input.id}`);
    return { deleted: true };
  },
});
"#;

const TODO_SCHEMA_TS: &str = r#"import { defineSchema, column, index } from "@flux/schema"

export default defineSchema({
  table:       "todos",
  description: "Todo items",
  timestamps:  true,  // auto-adds created_at + updated_at

  columns: {
    id:    column.uuid().primaryKey().default("gen_random_uuid()"),
    title: column.text().notNull(),
    done:  column.boolean().notNull().default(false),
  },

  indexes: [
    index(["done", "created_at"]).name("idx_todos_done_created"),
  ],
})
"#;

const TODO_ENV_EXAMPLE: &str = r#"GATEWAY_URL=https://YOUR_GATEWAY_URL
API_KEY=YOUR_API_KEY
PROJECT_ID=YOUR_PROJECT_ID
"#;

fn todo_readme(name: &str) -> String {
    format!(
        r#"# {name} — Todo API

A CRUD todo API built with Flux.

## Quick start

```bash
flux dev          # starts local server + embedded Postgres
flux login        # create admin account (first run)
flux db push      # apply schemas/ to Postgres
```

## Functions

| Function | Description |
|---|---|
| `create_todo` | Create a new to-do item |
| `list_todos` | List to-dos, with optional `done` filter |
| `update_todo` | Update title or mark done |
| `delete_todo` | Delete a to-do by ID |

## Invoke

```bash
flux invoke create_todo --payload '{{"title": "Buy groceries"}}'
flux invoke list_todos  --payload '{{"done": false}}'
flux invoke update_todo --payload '{{"id": "...", "done": true}}'
flux invoke delete_todo --payload '{{"id": "..."}}'
```

## Tracing

```bash
flux trace <x-request-id>
flux why   <x-request-id>   # root cause analysis
```
"#,
        name = name
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// webhook-worker — source files
// ─────────────────────────────────────────────────────────────────────────────

const WEBHOOK_TS: &str = r#"import { defineFunction, type FluxContext } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";
import { createHmac, timingSafeEqual } from "node:crypto";

/** Verify HMAC-SHA256 signature (Stripe, GitHub, and most providers). */
function verifySignature(raw: string, sig: string, secret: string): boolean {
  const expected = createHmac("sha256", secret).update(raw).digest("hex");
  const incoming = sig.replace(/^sha256=/, "");
  try {
    return timingSafeEqual(
      Buffer.from(expected, "hex"),
      Buffer.from(incoming, "hex"),
    );
  } catch {
    return false;
  }
}

export default defineFunction({
  name: "on_webhook",
  description: "Receive, verify, and persist webhook events",
  input: z.object({
    source:    z.string(),      // "stripe" | "github" | …
    signature: z.string(),      // X-Hub-Signature-256 / Stripe-Signature
    raw:       z.string(),      // raw JSON string body
  }),
  output: z.object({ received: z.boolean(), event_id: z.string() }),

  handler: async ({ input, ctx }) => {
    // 1. Verify signature
    const secretKey = `${input.source.toUpperCase()}_WEBHOOK_SECRET`;
    const secret = ctx.secrets.get(secretKey);
    if (!secret) throw new Error(`Secret ${secretKey} not set`);

    if (!verifySignature(input.raw, input.signature, secret)) {
      ctx.log("Webhook signature verification failed", "warn");
      throw new Error("Invalid signature");
    }

    const event = JSON.parse(input.raw);
    ctx.log(`Received ${input.source}/${event.type ?? "unknown"}`);

    const flux = createClient({
      url:       ctx.env.GATEWAY_URL,
      apiKey:    ctx.env.API_KEY,
      projectId: ctx.env.PROJECT_ID,
    });

    // 2. Persist the raw event
    const [record] = await flux.db.webhook_events
      .insert({
        source:     input.source,
        event_type: event.type ?? "unknown",
        payload:    event,
        processed:  false,
      })
      .returning(["id"])
      .execute();

    // 3. Dispatch handling (extend the switch for new event types)
    try {
      await handleEvent(input.source, event.type ?? "", event, ctx);
      await flux.db.webhook_events
        .update({ processed: true })
        .where("id", "eq", record.id)
        .execute();
    } catch (err) {
      // Log but don't fail — event is stored and can be retried
      ctx.log(`Event handler error: ${err}`, "error");
    }

    return { received: true, event_id: record.id };
  },
});

async function handleEvent(
  source: string,
  type: string,
  event: Record<string, unknown>,
  ctx: FluxContext,
): Promise<void> {
  switch (`${source}/${type}`) {
    case "stripe/payment_intent.succeeded":
      ctx.log(`Payment: ${(event.data as any)?.object?.amount}`);
      break;
    case "github/push":
      ctx.log(`Push to ${event.ref}`);
      break;
    default:
      ctx.log(`Unhandled: ${source}/${type}`, "warn");
  }
}
"#;

const WEBHOOK_SCHEMA_TS: &str = r#"import { defineSchema, column, index } from "@flux/schema"

export default defineSchema({
  table:       "webhook_events",
  description: "Incoming webhook events from external providers",
  timestamps:  false,

  columns: {
    id:         column.uuid().primaryKey().default("gen_random_uuid()"),
    source:     column.text().notNull(),
    event_type: column.text().notNull(),
    payload:    column.jsonb().notNull().default("{}"),
    processed:  column.boolean().notNull().default(false),
    created_at: column.timestamptz().notNull().default("now()"),
  },

  indexes: [
    index(["source", "event_type", "processed"]).name("idx_webhooks_source_type"),
    index(["processed", "created_at"]).name("idx_webhooks_retry"),
  ],
})
"#;

const WEBHOOK_ENV_EXAMPLE: &str = r#"GATEWAY_URL=https://YOUR_GATEWAY_URL
API_KEY=YOUR_API_KEY
PROJECT_ID=YOUR_PROJECT_ID

# Source-specific webhook secrets (add one per provider)
STRIPE_WEBHOOK_SECRET=whsec_...
GITHUB_WEBHOOK_SECRET=ghw_...
"#;

fn webhook_readme(name: &str) -> String {
    format!(
        r#"# {name} — Webhook Worker

Receive and process webhooks with signature verification backed by Flux.

## Function

| Function | Description |
|---|---|
| `on_webhook` | Verify, store, and dispatch incoming webhook events |

## Setup

### 1. Apply the schema

Run `schema/webhook_events.sql` in your Flux dashboard → Schema → SQL Editor.

### 2. Set secrets

```bash
flux secrets set GATEWAY_URL              https://YOUR_GATEWAY_URL
flux secrets set API_KEY                  YOUR_API_KEY
flux secrets set PROJECT_ID               YOUR_PROJECT_ID
flux secrets set STRIPE_WEBHOOK_SECRET    whsec_...   # optional
flux secrets set GITHUB_WEBHOOK_SECRET    ghw_...     # optional
```

### 3. Deploy

```bash
flux deploy
```

### 4. Configure your webhook provider

Point your provider to:
```
https://YOUR_GATEWAY/on_webhook
```

Send events in this format:
```json
{{
  "source": "stripe",
  "signature": "sha256=...",
  "raw": "{{\"type\":\"payment_intent.succeeded\",...}}"
}}
```

### 5. Retry failed events

Query `processed = false` and re-run from a cron function or manual invoke.

## Tracing

```bash
flux trace <x-request-id>
```
"#,
        name = name
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// ai-backend — source files
// ─────────────────────────────────────────────────────────────────────────────

const AI_TS: &str = r#"import { defineFunction } from "@flux/functions";
import { z } from "zod";
import { createClient } from "@flux/sdk";
import { createHash } from "node:crypto";

const SentimentEnum  = z.enum(["positive", "negative", "neutral"]);
const CategoryEnum   = z.enum(["support", "billing", "feedback", "spam", "other"]);

const ClassificationSchema = z.object({
  sentiment: SentimentEnum,
  category:  CategoryEnum,
  summary:   z.string().max(200),
});

export default defineFunction({
  name: "classify_text",
  description: "Classify text using OpenAI with DB-backed result caching",
  input: z.object({
    text: z.string().min(1).max(4000),
  }),
  output: ClassificationSchema.extend({
    cached: z.boolean(),
    tokens: z.number().int(),
    model:  z.string(),
  }),

  handler: async ({ input, ctx }) => {
    const flux = createClient({
      url:       ctx.env.GATEWAY_URL,
      apiKey:    ctx.env.API_KEY,
      projectId: ctx.env.PROJECT_ID,
    });

    // 1. Check cache
    const hash = createHash("sha256").update(input.text).digest("hex");
    const [cached] = await flux.db.classifications
      .where("content_hash", "eq", hash)
      .limit(1)
      .execute();

    if (cached) {
      ctx.log(`Cache hit (hash ${hash.slice(0, 8)})`);
      return {
        sentiment: cached.sentiment as z.infer<typeof SentimentEnum>,
        category:  cached.category  as z.infer<typeof CategoryEnum>,
        summary:   "",
        cached:    true,
        tokens:    cached.tokens,
        model:     cached.model,
      };
    }

    // 2. Call OpenAI
    const apiKey = ctx.secrets.get("OPENAI_API_KEY");
    if (!apiKey) throw new Error("OPENAI_API_KEY secret not set");

    const model = "gpt-4o-mini";
    ctx.log(`Classifying with ${model}`);

    const res = await fetch("https://api.openai.com/v1/chat/completions", {
      method:  "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${apiKey}` },
      body: JSON.stringify({
        model,
        response_format: { type: "json_object" },
        messages: [
          {
            role:    "system",
            content:
              'Classify the text. Reply with JSON: ' +
              '{"sentiment": "positive|negative|neutral", ' +
              '"category": "support|billing|feedback|spam|other", ' +
              '"summary": "<one sentence max 200 chars>"}',
          },
          { role: "user", content: input.text },
        ],
      }),
    });

    if (!res.ok) throw new Error(`OpenAI error: ${res.status}`);

    const data   = await res.json();
    const parsed = ClassificationSchema.parse(JSON.parse(data.choices[0].message.content));
    const tokens = data.usage.total_tokens as number;

    ctx.log(`Done: ${parsed.sentiment}/${parsed.category} tokens=${tokens}`);

    // 3. Persist for future cache hits
    await flux.db.classifications
      .insert({ content_hash: hash, input: input.text, ...parsed, model, tokens })
      .execute();

    return { ...parsed, cached: false, tokens, model };
  },
});
"#;

const AI_SCHEMA_TS: &str = r#"import { defineSchema, column, index } from "@flux/schema"

export default defineSchema({
  table:       "classifications",
  description: "LLM classification results — DB-backed cache",
  timestamps:  false,

  columns: {
    id:           column.uuid().primaryKey().default("gen_random_uuid()"),
    content_hash: column.text().notNull().unique(),
    input:        column.text().notNull(),
    sentiment:    column.text().notNull(),
    category:     column.text().notNull(),
    summary:      column.text().notNull().default("''"),
    model:        column.text().notNull(),
    tokens:       column.int().notNull().default(0),
    created_at:   column.timestamptz().notNull().default("now()"),
  },

  indexes: [
    index(["content_hash"]).unique().name("idx_classifications_hash"),
    index(["category", "created_at"]).name("idx_classifications_category"),
  ],
})
"#;

const AI_ENV_EXAMPLE: &str = r#"GATEWAY_URL=https://YOUR_GATEWAY_URL
API_KEY=YOUR_API_KEY
PROJECT_ID=YOUR_PROJECT_ID
OPENAI_API_KEY=sk-...
"#;

fn ai_readme(name: &str) -> String {
    format!(
        r#"# {name} — AI Backend

Text classification using OpenAI GPT-4o-mini, with automatic DB-backed caching.
Identical inputs never hit the LLM twice.

## Function

| Function | Description |
|---|---|
| `classify_text` | Classify text as sentiment + category, cached by content hash |

## Setup

### 1. Apply the schema

Run `schema/classifications.sql` in your Flux dashboard → Schema → SQL Editor.

### 2. Set secrets

```bash
flux secrets set GATEWAY_URL  https://YOUR_GATEWAY_URL
flux secrets set API_KEY      YOUR_API_KEY
flux secrets set PROJECT_ID   YOUR_PROJECT_ID
flux secrets set OPENAI_API_KEY sk-...
```

### 3. Deploy

```bash
flux deploy
```

### 4. Invoke

```bash
# First call — hits OpenAI (~ 1–2s)
flux invoke classify_text --payload '{{"text": "Your product is amazing!"}}'
# → {{"sentiment":"positive","category":"feedback","cached":false,"tokens":72,...}}

# Second call — instant cache hit (< 10ms)
flux invoke classify_text --payload '{{"text": "Your product is amazing!"}}'
# → {{"sentiment":"positive","category":"feedback","cached":true,...}}
```

## Tracing

```bash
flux trace <x-request-id>
```

The trace shows OpenAI latency on a miss and near-zero latency on a cache hit.
"#,
        name = name
    )
}
