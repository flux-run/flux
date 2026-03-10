# flux — Fluxbase CLI Reference

`flux` is the terminal interface for Fluxbase. It gives developers full control over every layer of the platform — from deploying a function and wiring a gateway route to managing database schema, running AI agents, and inspecting end-to-end traces — all without leaving the terminal.

**Every request in Fluxbase receives a unique request ID.** This ID links logs, traces, tool calls, database operations, and workflows together — enabling one-command debugging with `flux debug`.

- `flux debug` — interactive production debugger: shows recent errors, you pick one, it auto-runs trace + logs + suggests a fix
- `flux debug <request-id>` — deep-dive a specific request directly
- `flux tail` — live request stream (htop for your backend)

**Design principles:**
- `flux <resource> <operation>` — noun-first, verb-second
- Flags over positional args
- Every command is scriptable (`--output json`, `--confirm`, `--dry-run`)
- Destructive commands require confirmation unless `--confirm` is passed
- Context (tenant + project) is stored in `~/.fluxbase/config.json` and overridable per project via `.fluxbase/config.json`

---

## 30-second quickstart

New to Fluxbase? This shows the full flow from zero to a deployed, debuggable backend function.

```bash
# 1. Authenticate
flux login

# 2. Scaffold a project
flux new demo
cd demo

# 3. Create a function
flux function create create_user
cd create_user && npm install
# edit index.ts ...

# 4. Deploy
flux deploy

# 5. Call it through the gateway
curl https://gateway.fluxbase.co/signup \
  -H 'Content-Type: application/json' \
  -d '{"name":"Ada","email":"ada@example.com"}'

# 6. Watch live traffic and debug errors — two commands tell the whole story
flux tail                          # stream requests in real time
flux debug                         # interactive: pick an error, auto-debug it
flux debug <request-id-from-response>  # or jump directly to a known request
```

---

## Status legend

| Symbol | Meaning |
|--------|---------|
| ✅ | Implemented in CLI source |
| 🔧 | Partial / scaffold exists |
| 📋 | Planned, not yet built |

---

## Global flags

Apply to every command.

| Flag | Default | Description |
|------|---------|-------------|
| `--tenant <slug>` | from config | Override active tenant for this command |
| `--project <slug>` | from config | Override active project for this command |
| `--env <name>` | `production` | Target environment |
| `--output <format>` | `table` | `table \| json \| yaml \| plain` |
| `--json` | — | Shorthand for `--output json` |
| `--no-color` | — | Disable color output (useful for CI) |
| `--quiet` | — | Suppress non-error output |
| `--verbose` | — | Print HTTP requests and raw responses |
| `--dry-run` | — | Show what would happen without executing |
| `--confirm` | — | Skip confirmation prompts (for CI/CD) |
| `--version` | — | Print CLI version and exit |

**Environment variable overrides:**

| Env var | Overrides |
|---------|-----------|
| `FLUXBASE_API_URL` | API base URL |
| `FLUXBASE_GATEWAY_URL` | Gateway base URL |
| `FLUXBASE_RUNTIME_URL` | Runtime base URL |
| `FLUXBASE_TENANT_ID` | Active tenant |
| `FLUXBASE_PROJECT_ID` | Active project |

---

## Exit codes

All `flux` commands use consistent exit codes. Scripts and CI/CD pipelines can rely on these.

| Code | Meaning |
|------|---------|
| `0` | Success |
| `1` | CLI error (bad flags, missing argument, local I/O) |
| `2` | API error (non-2xx response from Fluxbase API) |
| `3` | Authentication failure (missing or expired token) |
| `4` | Resource not found (function, tenant, project, trace) |
| `5` | Conflict (e.g. resource already exists) |

```bash
# Example: check exit code in a script
flux deploy
if [ $? -ne 0 ]; then
  echo "Deploy failed"
  exit 1
fi
```

CI note: use `--confirm` to suppress prompts and `--no-color` for clean log output.

---

## Short aliases

Power-user shortcuts. All flags and subcommands work identically — aliases are purely ergonomic.

| Alias | Full command | Notes |
|-------|-------------|-------|
| `flux d` | `flux deploy` | deploy current context |
| `flux l` | `flux logs` | stream logs |
| `flux t` | `flux trace` | inspect a trace |
| `flux i` | `flux invoke` | invoke a function |
| `flux fn` | `flux function` | function subcommands |
| `flux db` | `flux db` | database subcommands (already short) |

For CI/CD scripts prefer the full names so scripts remain readable.

---

## Configuration files

### `~/.fluxbase/config.json` — global auth context

Stored after `flux login`. Never committed to version control.

```json
{
  "api_url": "https://api.fluxbase.co",
  "gateway_url": "https://fluxbase-gateway-658415624069.asia-south1.run.app",
  "runtime_url": "http://localhost:8083",
  "token": "flux_live_...",
  "tenant_id": "5b5f77d1-ce22-4439-8d81-b35c9ecb292e",
  "tenant_slug": "acme-org",
  "project_id": "3787e1fa-8a05-4c15-9dfc-d1c2a1bccc12"
}
```

### `.fluxbase/config.json` — per-project overrides

Committed to version control. Overrides global config for URL and project context.

```json
{
  "project_id": "3787e1fa-8a05-4c15-9dfc-d1c2a1bccc12",
  "api_url": "http://localhost:8080",
  "gateway_url": "http://localhost:8081",
  "runtime_url": "http://localhost:8083",
  "sdk_output": "src/fluxbase.generated.ts",
  "watch_interval": 5
}
```

### `flux.json` — per-function manifest

Lives in every function directory. Committed to version control.

```json
{
  "name": "send_email",
  "runtime": "deno",
  "entry": "index.ts",
  "description": "Send a welcome email via Composio Gmail"
}
```

---

## Complete command tree

```
flux
├── (no subcommand)                📋 launch interactive REPL shell
├── login                          ✅ authenticate with an API key
├── status                         📋 show active context + platform health
├── init                           ✅ initialise .fluxbase/config.json
├── new <name>                     ✅ scaffold a new project from a template
├── dev                            ✅ run local dev server
├── deploy                         ✅ deploy current function / all functions
├── rollback <name> --version <n>  ✅ roll back a function to a previous version
│
├── tenant
│   ├── create <name>              ✅
│   ├── list                       ✅
│   └── use <id>                   ✅
│
├── project
│   ├── create <name>              ✅
│   ├── list                       ✅
│   ├── get                        📋
│   ├── use <id>                   ✅
│   └── delete                     📋
│
├── function
│   ├── create <name>              ✅
│   ├── list                       ✅
│   ├── get <name>                 📋
│   ├── invoke <name>              ✅ (also: flux invoke <name>)
│   ├── logs <name>                📋 (also: flux logs function <name>)
│   └── delete <name>              📋
│
├── version                        ← deployment versioning
│   ├── list <function>            ✅ (also: flux deployments list <name>)
│   ├── get <function> --version   📋
│   ├── rollback <function>        ✅ (also: flux rollback)
│   ├── promote <function>         📋
│   └── diff <function>            📋
│
├── gateway                        ← HTTP routing layer
│   ├── route
│   │   ├── create                 📋
│   │   ├── list                   📋
│   │   ├── get <id>               📋
│   │   └── delete <id>            📋
│   ├── middleware
│   │   ├── add                    📋
│   │   └── remove                 📋
│   ├── rate-limit
│   │   ├── set                    📋
│   │   └── remove                 📋
│   └── cors
│       ├── set                    📋
│       └── list                   📋
│
├── db
│   ├── create [name]              ✅
│   ├── list                       ✅
│   ├── table
│   │   ├── create                 ✅
│   │   ├── list                   ✅
│   │   ├── describe               📋
│   │   └── delete                 📋
│   ├── column
│   │   ├── add                    📋
│   │   ├── update                 📋
│   │   └── drop                   📋
│   ├── index
│   │   ├── create                 📋
│   │   └── drop                   📋
│   ├── constraint
│   │   ├── add                    📋
│   │   └── drop                   📋
│   ├── row
│   │   ├── insert                 📋
│   │   ├── update                 📋
│   │   └── delete                 📋
│   ├── query                      📋
│   ├── shell                      📋 (interactive psql session)
│   ├── diff [env1] [env2]         📋 compare schemas between environments
│   └── migration
│       ├── create                 📋
│       ├── apply                  📋
│       ├── rollback               📋
│       └── status                 📋
│
├── tool
│   ├── list                       📋
│   ├── search <query>             📋
│   ├── describe <tool>            📋
│   ├── connect <app>              📋
│   ├── disconnect <app>           📋
│   └── run <action>               📋
│
├── workflow
│   ├── create <name>              📋
│   ├── list                       📋
│   ├── get <name>                 📋
│   ├── deploy <name>              📋
│   ├── run <name>                 📋
│   ├── logs <name>                📋
│   ├── trace <name>               📋
│   └── delete <name>              📋
│
├── agent
│   ├── create <name>              📋
│   ├── list                       📋
│   ├── get <name>                 📋
│   ├── deploy <name>              📋
│   ├── run <name>                 📋
│   ├── simulate <name>            📋
│   ├── trace <name>               📋 step-by-step reasoning trace
│   └── delete <name>              📋
│
├── schedule
│   ├── create                     📋
│   ├── list                       📋
│   ├── pause <name>               📋
│   ├── resume <name>              📋
│   ├── run <name>                 📋
│   ├── history <name>             📋
│   └── delete <name>              📋
│
├── queue
│   ├── create <name>              📋
│   ├── list                       📋
│   ├── describe <name>            📋
│   ├── publish <name>             📋
│   ├── bind <name>                📋
│   ├── bindings <name>            📋
│   ├── purge <name>               📋
│   ├── delete <name>              📋
│   └── dlq
│       ├── list <name>            📋
│       └── replay <name>          📋
│
├── event
│   ├── publish <type>             📋
│   ├── subscribe <type>           📋
│   ├── unsubscribe <id>           📋
│   ├── list                       📋
│   └── history <type>             📋
│
├── trace
│   ├── get <request-id>           ✅ (also: flux trace <id>)
│   ├── live                       📋
│   ├── search                     📋 --function --error --since
│   ├── replay <request-id>        📋 --payload <file> for override
│   └── export <request-id>        📋
│
├── logs                           ✅
│   Flags: --function, --workflow, --agent, --level, --since, --tail, --request-id
│
├── monitor
│   ├── status                     📋
│   ├── metrics                    📋
│   └── alerts
│       ├── create                 📋
│       ├── list                   📋
│       └── delete <id>            📋
│
├── secrets
│   ├── set <key> <value>          ✅
│   ├── get <key>                  📋
│   ├── list                       ✅
│   ├── delete <key>               ✅
│   └── import --from <file>       📋
│
├── env
│   ├── list                       📋
│   ├── create <name>              📋
│   ├── delete <name>              📋
│   ├── use <name>                 📋
│   └── clone <src> <dst>          📋
│
├── api-key
│   ├── create                     📋
│   ├── list                       📋
│   ├── revoke <id>                📋
│   └── rotate <id>                📋
│
├── sdk
│   ├── generate                   📋
│   └── (pull / watch / status)    ✅ (also: flux pull / flux watch / flux status)
│
├── debug [request-id]             ✅ interactive debugger (no args) or deep-dive a specific request
│     No args: lists recent errors → select one → auto trace + logs + suggested fix
│     With ID: direct deep-dive (trace + logs + suggested fix + optional replay)
├── tail [function]                ✅ live request stream — htop for your backend
│     Flags: --errors, --slow <ms>, --json
├── open [resource]                📋 open in browser
├── whoami                         📋 print current user + active context
├── doctor                         ✅
├── upgrade                        📋 self-update CLI to latest version
├── help [command]                 built-in print usage for any command
├── config
│   ├── list                       📋 show all config values
│   ├── set <key> <value>          📋 write a config value
│   └── reset                      📋 restore defaults
├── stack                          ✅
│   ├── up
│   ├── down
│   ├── reset                      📋 wipe and recreate local state
│   ├── seed                       📋 populate with fixture data
│   ├── status
│   └── logs
└── completion <shell>             📋 bash | zsh | fish
```

---

## Command reference

### `flux login` ✅

Authenticate the CLI with a Fluxbase API key. Keys are issued from the dashboard under **Settings → API Keys**.

```
flux login
```

Prompts for an API key (input hidden). Verifies against `/auth/me`, stores token + tenant/project context in `~/.fluxbase/config.json`.

**API key format:** must begin with `flux_`

```
$ flux login
Enter API Key: ••••••••••••••••••••
✔ Authenticated as user@example.com
✔ Auto-selected tenant: 5b5f77d1-...
✔ Auto-selected project: 3787e1fa-...
Login successful!
```

---

### `flux init` ✅

Initialise `.fluxbase/config.json` for the current project directory. Run once after cloning a repo.

```
flux init [flags]
```

| Flag | Description |
|------|-------------|
| `--project <id>` | Fluxbase project ID |
| `--output <file>` | Default SDK output path (default: `fluxbase.generated.ts`) |
| `--interval <secs>` | Watch interval for `flux watch` (default: `5`) |
| `--api-url <url>` | Override API URL (e.g. `http://localhost:8080` for local dev) |
| `--gateway-url <url>` | Override gateway URL |
| `--runtime-url <url>` | Override runtime URL |

```
$ flux init --project 3787e1fa
✔ Created .fluxbase/config.json
```

---

### `flux new <name>` ✅

Scaffold a new Fluxbase project from an official template.

> Renamed from `flux create` to follow the convention of `cargo new`, `npm create`,
> and `next create`. The name `create` is reserved as a generic subcommand verb
> across resource groups (`flux tenant create`, `flux db table create`, etc.).

```
flux new <name> [--template <template>]
```

| Flag | Description |
|------|-------------|
| `--template <name>` | `todo-api \| webhook-worker \| ai-backend` — omit to pick interactively |

```
$ flux new my-app
$ flux new my-app --template ai-backend
```

---

### `flux status` 📋

Show the active context and a health summary.

```
flux status
```

```
Context
  tenant:   acme-org  (5b5f77d1-...)
  project:  backend   (3787e1fa-...)
  env:      production

Functions
  create_user    v7   deployed   2h ago
  send_email     v3   deployed   5d ago

Gateway Routes
  POST /signup   → create_user (v7)
  POST /login    → auth_handler (v2)

Scheduled Jobs
  daily-cleanup  cron: "0 2 * * *"  next: 2026-03-11 02:00 UTC

Recent Errors (last 1h)
  3 errors in create_user
  → flux logs --function create_user --level error
```

---

### `flux dev` ✅

Start a local development server. Hot-reloads functions on file save.

```
flux dev [flags]
```

| Flag | Description |
|------|-------------|
| `--port <n>` | Port for local runtime (default: `8083`) |
| `--function <name>` | Only run a specific function |
| `--watch` | Watch for file changes (default: on) |

```
$ flux dev
  Runtime listening on http://localhost:8083
  Watching: send_email/, create_user/
  → flux invoke send_email --payload '{"email":"a@b.com"}'
```

---

### `flux deploy` ✅

Deploy the current directory. Behaviour depends on context:

- **In a function directory** (has `flux.json`): deploys that single function
- **At project root**: discovers all subdirectories with `flux.json` and deploys all

During deployment Flux automatically bundles your function and its dependencies,
compiles runtime-compatible JavaScript, and uploads the bundle to the Fluxbase
runtime. This ensures deterministic deployments regardless of the developer's
local environment.

```
flux deploy [flags]
```

| Flag | Description |
|------|-------------|
| `--name <n>` | Override function name (single-function mode) |
| `--runtime <r>` | Override runtime (single-function mode) |
| `--dry-run` | Show what would be deployed without uploading |
| `--env <name>` | Target environment |

```
$ cd send_email && flux deploy
  Bundling send_email...
  ✔ Deployed send_email v4  (1.2s)

$ cd .. && flux deploy
  Bundling create_user...   ✔ v7
  Bundling send_email...    ✔ v4
  Bundling auth_handler...  ✔ v2
  Deployed 3 functions
```

---

### `flux rollback <name> --version <n>` ✅

Activate a previous deployment version of a function.

```
flux rollback <function-name> --version <n>
```

```
$ flux rollback send_email --version 3
✔ Rolled back send_email to v3
```

> **Note:** Rollbacks take effect immediately on the gateway. All new incoming
> requests are routed to the restored version as soon as the rollback completes.

---

### `flux invoke <name>` ✅

Invoke a deployed function and print the result.

```
flux invoke <name> [flags]
```

| Flag | Description |
|------|-------------|
| `--payload <json>` | JSON payload to pass to the function |
| `--gateway` | Route through the gateway (applies auth + rate-limiting) |

```
$ flux invoke create_user --payload '{"name":"Ada","email":"ada@example.com"}'
{"ok":true,"email":"ada@example.com"}

$ flux invoke send_email --payload ./fixtures/test.json --gateway
```

---

### `flux tenant` ✅

Manage organizations.

#### `flux tenant create <name>`

```
$ flux tenant create "Acme Inc"
✔ Tenant created
  id:   5b5f77d1-...
  slug: acme-inc
✔ Now using tenant: 5b5f77d1-...
```

#### `flux tenant list`

```
$ flux tenant list
ID                                     NAME            ROLE
5b5f77d1-...                           Acme Inc        owner
8c3a2d44-...                           Side Project    admin
```

#### `flux tenant use <id>`

```
$ flux tenant use acme-inc
Now using tenant: acme-inc
```

---

### `flux project` ✅

Manage projects within a tenant.

```
flux project create <name>
flux project list
flux project use <id>
```

```
$ flux project create backend
✔ Project created: backend (3787e1fa-...)
✔ Now using project: 3787e1fa-...

$ flux project list
ID                                     NAME       TENANT
3787e1fa-...                           backend    acme-inc
```

---

### `flux function` ✅ / 📋

Manage serverless functions.

#### `flux function create <name>` ✅

Scaffolds a new function directory with `flux.json`, `package.json`, and `index.ts`.

```
$ flux function create send_email
✅ Created function 'send_email'

  cd send_email
  npm install
  flux deploy
  flux invoke send_email
```

**Generated `index.ts`:**
```typescript
import { defineFunction } from "@fluxbase/functions"
import { z } from "zod"

const Input = z.object({ name: z.string() })
const Output = z.object({ message: z.string() })

export default defineFunction({
  name: "send_email",
  description: "A simple hello-world function",
  input: Input,
  output: Output,
  handler: async ({ input, ctx }) => {
    ctx.log("Executing send_email handler")
    return { message: `Hello ${input.name}` }
  },
})
```

#### `flux function list` ✅

```
$ flux function list
NAME            RUNTIME   VERSION   STATUS     UPDATED
create_user     deno      v7        deployed   2h ago
send_email      deno      v3        deployed   5d ago
```

> **Single-function deploy:** to deploy just one function, `cd` into its
> directory and run `flux deploy`. Keeping deploy context-driven avoids a
> second mental model for the same operation.

#### `flux function delete <name>` 📋

```
$ flux function delete send_email
  This will permanently delete 'send_email' (v3).
  Gateway route POST /signup references this function.
  Type the function name to confirm: send_email
✔ Deleted
```

---

### `flux version` ✅ / 📋

Manage function deployment versions. `flux deployments list` is the current
implementation; `flux version` is the intended final surface.

```
flux version list <function>
flux version get <function> --version <n>
flux version rollback <function> [--to <n>]
flux version promote <function> --version <n> --to <env>
flux version diff <function> --from <n> --to <m>
```

```
$ flux version list send_email
ID                                     VERSION   STATUS      CREATED_AT
7a32f85d-...                           v7        active      2026-03-10 14:02
6c19a3e1-...                           v6        inactive    2026-03-09 11:44

$ flux version rollback send_email --to 6
✔ Rolled back send_email to v6
```

---

### `flux gateway` 📋

Manage HTTP routing between the public internet and your functions.

#### `flux gateway route create`

```
flux gateway route create \
  --path /signup \
  --method POST \
  --function create_user \
  --auth none
```

| Flag | Description |
|------|-------------|
| `--path <path>` | URL path (e.g. `/signup`) |
| `--method <verb>` | `GET \| POST \| PUT \| DELETE \| PATCH` |
| `--function <name>` | Target function name |
| `--auth <type>` | `none \| bearer \| api-key` |
| `--async` | Fire-and-forget (queue the call, return 202 immediately) |

#### `flux gateway route list`

```
$ flux gateway route list
ID           METHOD   PATH         FUNCTION       AUTH    ASYNC
73a5b7ce-…   POST     /signup      create_user    none    false
a1b2c3d4-…   POST     /login       auth_handler   none    false
```

#### `flux gateway middleware add`

```
flux gateway middleware add \
  --route 73a5b7ce \
  --type rate-limit \
  --config '{"rps":100,"burst":200}'
```

#### `flux gateway cors set`

```
flux gateway cors set \
  --route 73a5b7ce \
  --origins "https://app.example.com,https://staging.example.com"
```

---

### `flux db` ✅ / 📋

Full database schema management backed by PostgreSQL (Neon).

#### `flux db create [name]` ✅

```
$ flux db create
✔ Database "default" created  schema: tenant_5b5f77d1_default

$ flux db create analytics
```

#### `flux db list` ✅

```
$ flux db list
DATABASE
default
analytics
```

#### `flux db table create` ✅

```
$ flux db table create users --database default

$ flux db table create users --columns '[
  {"name":"id",         "type":"uuid",        "primary_key":true, "default":"gen_random_uuid()"},
  {"name":"email",      "type":"text",        "nullable":false},
  {"name":"name",       "type":"text"},
  {"name":"created_at", "type":"timestamptz", "default":"now()"}
]'
```

#### `flux db table list` ✅

```
$ flux db table list
TABLE                          COLUMNS
users                          id, email, name, created_at
orders                         id, user_id, total, status, created_at
```

#### `flux db column add` 📋

```
$ flux db column add users phone_number text --nullable
$ flux db column drop users phone_number --confirm
```

#### `flux db index create` 📋

```
$ flux db index create users email --unique
$ flux db index drop users email
```

#### `flux db query` 📋

```
$ flux db query "SELECT * FROM users WHERE email = 'ada@example.com'"
$ flux db query --file ./queries/active_users.sql
```

#### `flux db shell` 📋

Open an interactive `psql` session against the project database.

```
$ flux db shell
psql (15.4)  connected to tenant_5b5f77d1_default
=#
```

#### `flux db diff [env1] [env2]` 📋

Compare schemas between two environments. Outputs a human-readable diff and
an optional migration SQL file.

| Flag | Description |
|------|-------------|
| `--format <fmt>` | `table \| sql \| json` (default: `table`) |
| `--output <file>` | Write migration SQL to a file |

```
$ flux db diff production staging
  TABLE     COLUMN           CHANGE
  users     stripe_customer  + added (text, nullable)
  orders    (missing)        + table added in staging

$ flux db diff production staging --format sql --output migration.sql
✔ Wrote migration.sql (3 statements)
```

#### `flux db migration create` 📋

```
$ flux db migration create add_stripe_customer_id
✔ Created migrations/20260310_000001_add_stripe_customer_id.sql
```

#### `flux db migration apply` 📋

```
$ flux db migration apply
  Applying 20260310_000001_add_stripe_customer_id.sql ... ✔
  1 migration applied
```

#### `flux db migration status` 📋

```
$ flux db migration status
VERSION              NAME                                  APPLIED
20260308_000001      init                                  ✔  2026-03-08 09:00
20260309_000002      add_users_table                       ✔  2026-03-09 11:00
20260310_000003      add_stripe_customer_id                ✗  (pending)
```

---

### `flux tool` 📋

Manage external tool integrations via Composio. Tools are called from functions
using `ctx.tools.run()`.

#### `flux tool list`

```
$ flux tool list
APP        ACTION                    DESCRIPTION
gmail      gmail.send_email          Send an email via Gmail
slack      slack.send_message        Post a message to a Slack channel
github     github.create_issue       Open a GitHub issue
```

#### `flux tool search <query>`

```
$ flux tool search "send email"
gmail.send_email
sendgrid.send_email
mailgun.send_email
```

#### `flux tool describe <action>`

```
$ flux tool describe gmail.send_email

  gmail.send_email
  Send an email from the connected Gmail account.

  Parameters:
    recipient_email   string   required
    subject           string   required
    body              string   required
    cc                string   optional

  Connected accounts:
    user_123  →  shashi@example.com  (active)
```

#### `flux tool connect <app>`

```
$ flux tool connect gmail
  Opening browser to connect your Gmail account...
  Waiting...
  ✔ Connected: gmail (entity: user_123)
```

#### `flux tool run <action>` 📋

Test a tool action directly from the terminal without writing a function.

```
$ flux tool run gmail.send_email \
    --param recipient_email=test@example.com \
    --param subject="Hello from flux" \
    --param body="Testing the CLI"
✔ gmail.send_email completed (1862ms)
```

---

### `flux workflow` 📋

Define and run multi-step orchestration workflows. Steps can be functions,
tools, delays, or conditionals.

```
flux workflow create <name>
flux workflow list
flux workflow get <name>
flux workflow deploy <name>
flux workflow run <name> [--payload <json>]
flux workflow logs <name>
flux workflow trace <name> --request-id <id>
flux workflow delete <name>
```

**Example workflow definition (`workflow.json`):**
```json
{
  "name": "onboarding",
  "steps": [
    { "name": "insert_user",    "type": "function", "function": "create_user" },
    { "name": "send_email",     "type": "tool",     "action": "gmail.send_email" },
    { "name": "wait",           "type": "delay",    "duration": "2h" },
    { "name": "send_follow_up", "type": "tool",     "action": "gmail.send_email",
      "condition": "steps.send_email.result.ok == true" }
  ]
}
```

```
$ flux workflow run onboarding --payload '{"name":"Ada","email":"ada@example.com"}'
  step insert_user    ✔  0ms
  step send_email     ✔  1862ms
  step wait           ⏳ scheduled for 2h
  request_id: 9624a58d57e7
```

---

### `flux agent` 📋

Define and run AI agents that can reason, plan, and call tools autonomously.

```
flux agent create <name>
flux agent list
flux agent get <name>
flux agent deploy <name>
flux agent run <name> [--input <text>]
flux agent simulate <name> [--scenario <file>]
flux agent delete <name>
```

`flux agent simulate` runs the agent against a fixture scenario and prints
the reasoning trace without making real tool calls — safe for testing.

`flux agent trace` replays the recorded reasoning trace for a past run — showing
every tool call attempted, which succeeded, and the final answer. Useful when
`simulate` passes but production behaviour diverges.

```
$ flux agent run support-bot --input "My order hasn't arrived"
  → tool: notion.search_page ("order not arrived policy")
  → tool: gmail.send_email (customer: ada@example.com)
  ✔ Done (3 steps, 4.2s)
  Result: "I've sent a follow-up email with tracking details."

$ flux agent simulate support-bot --scenario ./scenarios/missing_order.json

$ flux agent trace support-bot --request-id 9624a58d57e7
  step 1  notion.search_page       245ms  ✔  found 1 result
  step 2  gmail.send_email         1862ms ✔  sent to ada@example.com
  conclusion: "I've sent a follow-up email with tracking details."
```

---

### `flux schedule` 📋

Trigger functions or workflows on a cron schedule.

> **Design note:** schedules are time-based triggers on top of functions or
> workflows — conceptually they are `workflow trigger cron`. A future version
> may surface this as `flux workflow trigger --cron` for consistency, but the
> `flux schedule` namespace is kept for discoverability.

```
flux schedule create --name <name> --cron <expr> --function <name>
flux schedule list
flux schedule pause <name>
flux schedule resume <name>
flux schedule run <name>
flux schedule history <name>
flux schedule delete <name>
```

| Flag | Description |
|------|-------------|
| `--cron <expr>` | Standard 5-part cron expression |
| `--function <name>` | Function to trigger |
| `--workflow <name>` | Workflow to trigger (alternative to `--function`) |
| `--payload <json>` | Static payload to send on each trigger |

```
$ flux schedule create \
    --name daily-cleanup \
    --cron "0 2 * * *" \
    --function cleanup_old_sessions
✔ Scheduled: daily-cleanup  next run: 2026-03-11 02:00 UTC

$ flux schedule history daily-cleanup
RUN ID         STATUS    STARTED              DURATION
abc123         success   2026-03-10 02:00     1.2s
ghi789         error     2026-03-08 02:00     0.1s
```

---

### `flux queue` 📋

> **queue vs event** — `queue` is a **work queue**: each message is consumed
> by exactly one function instance, with retry and dead-letter support. Use it
> for jobs that must run once (emails, billing, image processing).
> `event` is a **pub/sub bus**: every subscriber receives a copy of the event.
> Use it for fan-out notifications and audit streams.

Manage async message queues. Functions bind as consumers and process messages
with retry and dead-letter support.

```
flux queue create <name> [--max-retries <n>] [--visibility-timeout <duration>]
flux queue list
flux queue describe <name>
flux queue publish <name> --payload <json>
flux queue bind <name> --function <fn>
flux queue bindings <name>
flux queue purge <name>
flux queue delete <name>
flux queue dlq list <name>
flux queue dlq replay <name>
```

```
$ flux queue create email-jobs --max-retries 3
$ flux queue bind email-jobs --function process_email
$ flux queue publish email-jobs --payload '{"to":"ada@example.com"}'
✔ Published (message_id: msg_8f3a2b...)

$ flux queue dlq list email-jobs
MESSAGE ID       ATTEMPTS   LAST ERROR              LAST ATTEMPT
msg_deadbeef     3          "invalid email format"  2026-03-10 08:12
```

---

### `flux event` 📋

> See the **queue vs event** note above. `event` delivers to all subscribers;
> `queue` delivers to exactly one consumer.

Pub/sub event bus. Functions subscribe to event types and are invoked when
matching events are published.

```
flux event publish <type> --payload <json>
flux event subscribe <type> --function <name>
flux event unsubscribe <subscription-id>
flux event list
flux event history <type> [--since <duration>]
```

```
$ flux event subscribe user.signed_up --function send_welcome_email
✔ Subscribed: user.signed_up → send_welcome_email (sub_9f3a...)

$ flux event publish user.signed_up --payload '{"user_id":"123","email":"ada@example.com"}'
✔ Published (event_id: evt_abc123)

$ flux event history user.signed_up --since 1h
EVENT ID      TYPE             PUBLISHED AT           TRIGGERED
evt_abc123   user.signed_up   2026-03-10 14:01       1
```

---

### `flux trace <request-id>` ✅

Show the full cross-service execution trace for a request.

Every request in Fluxbase generates a trace automatically — no instrumentation
or manual span creation is required.

```
flux trace <request-id> [flags]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--slow <ms>` | `500` | Highlight spans slower than this threshold |
| `--flame` | false | Render a waterfall timeline instead of a table |

```
$ flux trace 9624a58d57e7

  request: 9624a58d57e7   status: ready   total: 3816ms

  TIME          SOURCE     SPAN                     DURATION   DELTA
  14:01:12.031  gateway    gateway.receive          11ms       —
  14:01:12.041  gateway    gateway.route            487ms      +476ms
  14:01:12.528  function   create_user              146ms      +146ms
  14:01:12.674  db         db.insert(users)         0ms        —
  14:01:12.674  tool       gmail.send_email         1862ms     +1862ms ⚠

$ flux trace 9624a58d57e7 --flame
  14:01:12.031  ┤ gateway.receive (11ms)
  14:01:12.041  ┤──────────────────── gateway.route (487ms)
  14:01:12.528  ┤ create_user (146ms)
  14:01:12.674  ┤ db.insert(users) (0ms)
  14:01:12.674  ┤──────────────────────────────────────── gmail.send_email (1862ms)
```

#### `flux trace live` 📋

Stream traces for all incoming requests in real time.

```
$ flux trace live
  [14:01:12]  POST /signup  9624a58d  →  create_user  3.8s  ✔
  [14:01:45]  POST /signup  a3b7c1e2  →  create_user  2.1s  ✔
  [14:02:01]  POST /signup  f8e3d9c4  →  create_user  0.1s  ✗  invalid_email
```

#### `flux trace search` 📋

Search historical traces with filters. Combines into a production debugging
session when used interactively.

| Flag | Description |
|------|-------------|
| `--function <name>` | Filter to traces involving this function |
| `--route <path>` | Filter by gateway route, e.g. `/signup` |
| `--status <status>` | `error \| success` |
| `--error` | Shorthand for `--status error` |
| `--tool <action>` | Filter traces that called a specific tool, e.g. `gmail.send_email` |
| `--since <duration>` | e.g. `1h`, `30m`, `24h` |
| `--min-duration <ms>` | Only traces slower than this threshold |

```
$ flux trace search --error
  REQUEST ID   ROUTE         FUNCTION      DURATION   ERROR
  9624a58d     POST /signup  create_user   3816ms     gmail_rate_limit
  b3c7d2e1     POST /login   auth_handler  102ms      invalid_password

$ flux trace search --function create_user --error --since 1h
  REQUEST ID   ROUTE         FUNCTION      DURATION   ERROR
  9624a58d     POST /signup  create_user   3816ms     gmail_rate_limit
  f8e3d9c4     POST /signup  create_user   102ms      invalid_email

$ flux trace search --tool gmail.send_email --since 24h
$ flux trace search --route /signup --min-duration 2000
```

#### `flux trace replay <request-id>` 📋

Re-execute a past request — same payload by default, or override with `--payload`
to test a small fix without hitting the original flow.

| Flag | Description |
|------|-------------|
| `--payload <file>` | JSON file to use instead of the original payload |
| `--dry-run` | Print the payload that would be sent without executing |

```
$ flux trace replay 9624a58d57e7
  new request_id: b1c2d3e4f5a6

$ flux trace replay 9624a58d57e7 --payload override.json
  using custom payload: override.json
  new request_id: c2d3e4f5a6b7
```

#### `flux trace export <request-id>` 📋

```
$ flux trace export 9624a58d57e7 --format json > trace.json
$ flux trace export 9624a58d57e7 --format otlp > trace.otlp.json
```

---

### `flux logs` ✅

Stream or fetch logs across all platform components.

```
flux logs [source] [resource] [flags]
```

| Flag | Default | Description |
|------|---------|-------------|
| `--follow` / `-f` | false | Stream live (poll every 1.5s) |
| `--limit <n>` | `100` | Number of lines to fetch |
| `--level <level>` | all | `debug \| info \| warn \| error` |
| `--since <duration>` | — | e.g. `1h`, `30m`, `24h` |
| `--request-id <id>` | — | Filter to a specific request |

**Sources:** `function` | `workflow` | `agent` | `db` | `queue` | `system`

```
$ flux logs                              # all logs, most recent 100
$ flux logs function create_user         # logs for a specific function
$ flux logs function create_user -f      # streaming
$ flux logs --level error --since 1h     # recent errors
$ flux logs --request-id 9624a58d57e7   # all logs for one request
```

---

### `flux secrets` ✅

Store encrypted secrets scoped to a project. Available inside functions as `ctx.env`.

```
flux secrets set <key> <value>
flux secrets list
flux secrets get <key>              📋
flux secrets delete <key>
flux secrets import --from <file>   📋
```

```
$ flux secrets set STRIPE_SECRET_KEY sk_live_...
Secret 'STRIPE_SECRET_KEY' set successfully.

$ flux secrets list
KEY                            UPDATED_AT                     VERSION
STRIPE_SECRET_KEY              2026-03-10 14:00               1
SENDGRID_API_KEY               2026-03-09 09:00               2

$ flux secrets import --from .env
✔ Imported 4 secrets from .env
```

---

### `flux env` 📋

Manage named environments (production, staging, preview). Each has its own
secrets and config.

```
flux env list
flux env create <name>
flux env delete <name>
flux env use <name>
flux env clone <source> <destination>
```

```
$ flux env create staging
✔ Environment 'staging' created

$ flux env clone production staging
✔ Cloned secrets: production → staging (8 secrets)
```

---

### `flux api-key` 📋

Manage programmatic API keys for CI/CD and service-to-service calls.

```
flux api-key create --name <name> [--scopes <scopes>]
flux api-key list
flux api-key revoke <id>
flux api-key rotate <id>
```

| Scope | Grants |
|-------|--------|
| `function:invoke` | Invoke functions via gateway |
| `function:deploy` | Deploy new function versions |
| `logs:read` | Read logs and traces |
| `secrets:write` | Create and update secrets |
| `admin` | Full access |

```
$ flux api-key create --name ci-pipeline --scopes "function:deploy,logs:read"
✔ key: flux_live_9f3a2b...  (store this — shown only once)
```

---

### `flux monitor` 📋

Platform observability: health checks, metrics, and alerts.

#### `flux monitor status`

```
$ flux monitor status
Service           Status    Latency (p50/p95)   Error Rate (1h)
fluxbase-api      healthy   45ms / 210ms        0.1%
fluxbase-runtime  healthy   320ms / 1800ms      0.3%
fluxbase-gateway  healthy   8ms / 42ms          0.0%
```

#### `flux monitor metrics`

```
$ flux monitor metrics --function create_user --window 1h
invocations:    142
success_rate:   98.6%
p50_duration:   312ms
p95_duration:   2100ms
errors:         2  (invalid_email: 1, timeout: 1)
```

#### `flux monitor alerts`

```
$ flux monitor alerts create \
    --name high-error-rate \
    --metric function_error_rate \
    --function create_user \
    --threshold 0.05 \
    --window 5m \
    --notify email
```

---

### `flux sdk` ✅ / 📋

Generate or synchronise the typed TypeScript SDK for the current project schema.

```
flux pull [--output <file>]          # ✅ download current schema as TypeScript
flux watch [--output <file>]         # ✅ auto-regenerate when schema changes
flux status [--sdk <file>]           # ✅ compare local vs remote schema version
flux sdk generate [--lang <lang>]    # 📋 multi-language generation
```

```
$ flux pull
✔ Generated src/fluxbase.generated.ts  (schema v5)

$ flux sdk generate --lang python --output ./sdk/fluxbase.py
```

---

### `flux open [resource]` 📋

Open the Fluxbase dashboard in the default browser.

```
flux open                          # dashboard home
flux open function send_email      # function detail page
flux open trace 9624a58d57e7       # trace viewer
flux open logs                     # log viewer
flux open gateway                  # gateway route list
```

---

### `flux doctor` ✅

Diagnose the developer environment.

```
$ flux doctor

Fluxbase CLI doctor
────────────────────────────────────────────────
✔  CLI version:       0.2.0
✔  API reachable:     https://api.fluxbase.co  (62ms)
✔  Authenticated:     user@example.com
✔  Tenant:            acme-org  (5b5f77d1-...)
✔  Project:           backend   (3787e1fa-...)
✔  SDK file:          src/fluxbase.generated.ts
     Schema:          v4  (hash: a3f8c1d2)  generated 2026-03-09T10:02:41Z
⚠  SDK outdated:      local v4 → remote v5  →  run: flux pull
```

Checks: CLI version, API reachability, authentication, active tenant/project,
`.fluxbase/config.json`, SDK version drift, Node.js availability.

---

### `flux stack` ✅

Manage the full local development stack via Docker Compose.

```
flux stack up       # start all services locally
flux stack down     # stop all services
flux stack reset    # 📋 wipe volumes and recreate from scratch
flux stack seed     # 📋 populate databases with fixture data
flux stack status   # show running containers
flux stack logs     # tail all service logs
```

`flux stack reset` wipes all Docker volumes and rebuilds the stack — useful
after a migration conflict or when you need a completely clean state.

`flux stack seed` runs seed scripts in `fixtures/` to populate local databases
with test data after `flux stack up` or `flux stack reset`.

Reads from `docker-compose.dev.yml` at the project root.

---

### `flux completion <shell>` 📋

Generate shell completion scripts.

```
flux completion bash  >> /etc/bash_completion.d/flux
flux completion zsh   >> ~/.zsh/completions/_flux
flux completion fish  >> ~/.config/fish/completions/flux.fish
```

---

### `flux whoami` 📋

Print the currently authenticated user and active context. The fastest way to
verify you are operating in the right tenant/project before a destructive
command.

```
$ flux whoami

  user:    shashi@example.com
  tenant:  acme-org   (5b5f77d1-...)
  project: backend    (3787e1fa-...)
  env:     production
  token:   flux_live_... (expires in 23h)
```

---

### `flux upgrade` 📋

Self-update the `flux` CLI binary to the latest released version. Follows the
pattern used by `supabase update`, `vercel update`, `stripe upgrade`.

```
$ flux upgrade

  Current version:  v0.2.0
  Latest version:   v0.3.1
  Downloading flux v0.3.1...
  ✔ Upgraded to v0.3.1

$ flux upgrade --version 0.2.8   # pin to a specific version
$ flux upgrade --check            # print latest without installing
```

---

### `flux --version`

Print the installed CLI version and the API version it targets. Available as
both a flag and a zero-arg invocation.

```
$ flux --version
flux CLI  v0.2.0
API       v2026-03
```

Note: `flux version` (without `--`) is the deployment versioning namespace
(`flux version list`, `flux version rollback`, etc.). Use `flux --version` to
query the CLI itself.

---

### `flux help [command]` built-in

Print usage information for any command or subcommand. Available automatically
via the CLI framework; documented here for discoverability.

```
$ flux help
$ flux help queue
$ flux help queue publish
$ flux help trace search

# Equivalent forms
$ flux queue --help
$ flux queue publish --help
```

Every flag, its default, and a one-line description is printed. Long
descriptions include an **Examples** block.

---

### `flux config` 📋

Inspect and modify the active configuration without editing JSON files manually.
Operates on the nearest config file: `.fluxbase/config.json` if present,
otherwise `~/.fluxbase/config.json`.

```
flux config list
flux config set <key> <value>
flux config reset
```

```
$ flux config list

  Source: ~/.fluxbase/config.json

  KEY             VALUE
  api_url         https://api.fluxbase.co
  gateway_url     https://fluxbase-gateway-658415624069.asia-south1.run.app
  runtime_url     http://localhost:8083
  tenant_id       5b5f77d1-...
  tenant_slug     acme-org
  project_id      3787e1fa-...

$ flux config set api_url http://localhost:8080
✔ Set api_url = http://localhost:8080 in .fluxbase/config.json

$ flux config reset
  This will restore all values to platform defaults.
  Confirm? [y/N]: y
✔ Reset .fluxbase/config.json
```

| Key | Default |
|-----|--------|
| `api_url` | `https://api.fluxbase.co` |
| `gateway_url` | `https://gateway.fluxbase.co` |
| `runtime_url` | `http://localhost:8083` |
| `tenant_id` | set by `flux login` |
| `project_id` | set by `flux project use` |

---

### `flux` (interactive REPL) 📋

Running `flux` with no arguments launches an interactive shell. Useful for
debugging sessions where you want to run multiple commands in sequence without
re-authenticating or re-loading context each time.

Inspired by `redis-cli`, `terraform console`, and `psql`.

```
$ flux

  Fluxbase CLI  v0.2.0
  tenant: acme-org  project: backend
  Type 'help' or '?' for commands, 'exit' to quit.

flux> trace search --error --since 1h
  REQUEST ID   ROUTE         FUNCTION      DURATION   ERROR
  9624a58d     POST /signup  create_user   3816ms     gmail_rate_limit

flux> debug 9624a58d
  [... full debug output ...]

flux> trace replay 9624a58d --payload override.json
  new request_id: c2d3e4f5a6b7

flux> logs --request-id c2d3e4f5a6b7 --follow
```

The REPL preserves context (tenant, project, auth token) across commands and
supports tab completion for subcommands and flags.

---

### `flux debug [request-id]` ✅

The **signature command** of Fluxbase. Two modes:

**Interactive mode** (`flux debug` — no args):  
Lists recent production errors. You select one. The CLI automatically runs trace + logs + suggests a fix.

**Direct mode** (`flux debug <request-id>`):  
Deep-dives a specific request — trace + logs + suggested fix + optional replay.

```
flux debug                         # interactive: pick from recent errors
flux debug <request-id> [flags]    # direct: inspect a known request
```

#### Interactive mode

```
$ flux debug

Recent Production Errors (last 10m)
────────────────────────────────────────────────────
1) POST /signup      create_user      gmail_rate_limit
   request_id: 9624a58d57e7   3.8s

2) POST /login       auth_handler     invalid_password
   request_id: a7f9c1e4d2ab   102ms

3) POST /signup      create_user      invalid_email
   request_id: f8e3d9c4a1b7   101ms

Select an error to inspect › 1

Inspecting 9624a58d57e7
```

The CLI then runs the full debug flow for the selected request (same output as direct mode below).

#### Direct mode

| Flag | Description |
|------|-------------|
| `--replay` | Automatically replay after showing the trace |
| `--replay-payload <file>` | Replay with an overridden payload |
| `--no-logs` | Skip the logs section |

```
$ flux debug 9624a58d57e7

Request Summary
────────────────────────────────────────
Request ID: 9624a58d57e7
Route:      POST /signup
Function:   create_user
Duration:   3816ms
Status:     error
Time:       2026-03-10 14:01:12 UTC

Trace
─────────────────────────────────────────────
gateway.receive      11ms    ✔
gateway.route        487ms   ✔
create_user          146ms   ✔
db.insert(users)     0ms     ✔
gmail.send_email     1862ms  ✗  rate_limit_exceeded

Logs
─────────────────────────────────────────────
[14:01:12.528]  create_user    INFO   sending welcome email to ada@example.com
[14:01:14.390]  gmail          ERROR  API rate limit exceeded (retry-after: 30s)

Suggested Fix
─────────────────────────────────────────────
⚠ gmail.send_email hit a rate limit.
  → Queue the email job instead of calling inline.
  → flux queue create email-jobs
  → flux queue bind email-jobs --function send_email

Replay this request? [y/N]: y
  new request_id: c2d3e4f5a6b7
```

This is the first thing a developer should reach for when something goes wrong in production. The 3-command story for Fluxbase is:

```bash
flux deploy           # ship your backend
flux tail             # watch live traffic
flux debug            # fix what breaks
```

---

### `flux tail [function]` ✅

Live request stream — **htop for your backend**. Streams requests as they arrive with method, route, function, duration, and pass/fail status. Errors print an inline `flux debug <id>` hint.

```
flux tail [function] [flags]
```

| Flag | Description |
|------|-------------|
| `--errors` | Show only failed requests |
| `--slow <ms>` | Show only requests slower than N ms |
| `--json` | Output raw JSON (one object per line, for piping) |

```
$ flux tail

Fluxbase · Live Request Stream
Watching: all requests
────────────────────────────────────────────────────────────────────────────
METHOD   ROUTE                         FUNCTION                DURATION   STATUS
────────────────────────────────────────────────────────────────────────────
POST     /signup                       create_user             312ms      ✔
GET      /status                       health_check            8ms        ✔
POST     /signup                       create_user             281ms      ✔
POST     /signup                       create_user             3.8s       ✗ rate_limit
   → flux debug 9624a58d57e7
POST     /login                        auth_handler            102ms      ✔
```

```bash
flux tail                      # all functions
flux tail create_user          # single function
flux tail --errors             # errors only — immediately actionable
flux tail --slow 500           # requests taking > 500ms
flux tail --json | jq .        # pipe to jq for custom filtering
```



---

## Developer workflow examples

### 1. First-time setup

```bash
flux login
flux tenant list
flux tenant use acme-inc
flux project list
flux project use backend
```

### 2. Build a new backend endpoint

```bash
flux function create send_email
cd send_email && npm install
# edit index.ts
flux deploy
flux invoke send_email --payload '{"to":"ada@example.com","subject":"Hello"}'
flux gateway route create --path /send-email --method POST --function send_email
flux logs function send_email -f
```

### 3. Debug a production error

```bash
# Option A: Interactive mode — lists recent errors, pick one, auto-debugs
flux debug

# Option B: You already have a request ID (from response header or flux tail)
flux debug 9624a58d57e7

# Watch live traffic to catch errors as they happen (Ctrl-C to stop)
flux tail
flux tail create_user          # filter to one function
flux tail --errors             # errors only
flux tail --slow 500           # only requests > 500ms

# Drill deeper after picking a request
flux trace 9624a58d57e7 --flame
flux trace search --function create_user --error --since 1h
flux logs --request-id 9624a58d57e7

# Replay with a patched payload to verify the fix
flux trace replay 9624a58d57e7 --payload override.json

# If the bug is a regression, roll back
flux version list send_email
flux rollback send_email --version 5
```

### 4. Add a database table

```bash
flux db table create orders --columns '[
  {"name":"id",         "type":"uuid",        "primary_key":true, "default":"gen_random_uuid()"},
  {"name":"user_id",    "type":"uuid",        "nullable":false},
  {"name":"total",      "type":"numeric(10,2)"},
  {"name":"status",     "type":"text",        "default":"pending"},
  {"name":"created_at", "type":"timestamptz", "default":"now()"}
]'
flux db index create orders user_id
flux db table describe orders
```

### 5. Connect an external tool

```bash
flux tool search gmail
flux tool describe gmail.send_email
flux tool connect gmail
flux tool run gmail.send_email \
  --param recipient_email=test@example.com \
  --param subject="Test" \
  --param body="Hello from flux"
```

### 6. Set up a scheduled job

```bash
flux function create cleanup_sessions
cd cleanup_sessions && npm install && flux deploy
flux schedule create \
  --name nightly-cleanup \
  --cron "0 2 * * *" \
  --function cleanup_sessions
flux schedule run nightly-cleanup   # manual test trigger
flux schedule history nightly-cleanup
```

### 7. CI/CD pipeline

```yaml
# .github/workflows/deploy.yml
name: Deploy to Fluxbase
on:
  push:
    branches: [main]
jobs:
  deploy:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: cargo install flux-cli
      - run: flux login && flux deploy --confirm
        env:
          FLUXBASE_API_KEY: ${{ secrets.FLUXBASE_API_KEY }}
          FLUXBASE_TENANT_ID: ${{ secrets.FLUXBASE_TENANT_ID }}
          FLUXBASE_PROJECT_ID: ${{ secrets.FLUXBASE_PROJECT_ID }}
```

---

## Function runtime context (`ctx`)

Every deployed function receives a `ctx` object injected by the runtime.

```typescript
export default defineFunction({
  handler: async ({ input, ctx }) => {

    // Logging — appears in flux logs
    ctx.log("message", "info")         // debug | info | warn | error

    // Secrets — set via flux secrets set
    const key = ctx.env.STRIPE_KEY

    // Tool execution — calls Composio-backed integrations
    await ctx.tools.run("gmail.send_email", {
      recipient_email: input.email,
      subject: "Welcome",
      body: "Hello!",
    })

    // Workflow orchestration
    await ctx.workflow.run([
      { name: "step-1", fn: async () => { /* ... */ } },
      { name: "step-2", fn: async () => { /* ... */ } },
    ])

    // Agent execution
    await ctx.agent.run("support-bot", { input: "user message" })
  }
})
```

---

## API routes backed by each CLI command

| CLI command | API endpoint |
|-------------|-------------|
| `flux login` | `GET /auth/me` |
| `flux tenant create` | `POST /tenants` |
| `flux tenant list` | `GET /tenants` |
| `flux project create` | `POST /projects` |
| `flux project list` | `GET /projects` |
| `flux function list` | `GET /functions` |
| `flux function create` | local scaffold only |
| `flux deploy` | `POST /functions/:name/deploy` (multipart) |
| `flux invoke` | `POST /runtime/invoke/:name` or gateway route |
| `flux rollback` | `POST /functions/:name/deployments/:version/activate` |
| `flux version list` | `GET /functions/:name/deployments` |
| `flux secrets list` | `GET /secrets` |
| `flux secrets set` | `POST /secrets` |
| `flux secrets delete` | `DELETE /secrets/:key` |
| `flux logs` | `GET /logs?source=...&limit=...` |
| `flux trace <id>` | `GET /traces/:id` |
| `flux trace search` | `GET /traces?function=...&error=true&since=...` |
| `flux trace replay` | `POST /traces/:id/replay` |
| `flux tail` | `GET /traces?limit=25&order=desc&since=<cursor>` (polled every 2s) |
| `flux debug` (interactive) | `GET /traces?status=error&limit=15&window=10m` |
| `flux debug <id>` | composite: `GET /traces/:id` + `GET /logs` + `POST /traces/:id/replay` |
| `flux db create` | `POST /db/databases` |
| `flux db list` | `GET /db/databases` |
| `flux db table create` | `POST /db/tables` |
| `flux db table list` | `GET /db/tables/:database` |
| `flux db diff` | `GET /db/diff?from=:env1&to=:env2` |
| `flux agent trace` | `GET /agents/:name/traces/:request_id` |
| `flux gateway route list` | `GET /gateway/routes` |
| `flux gateway route create` | `POST /gateway/routes` |
| `flux gateway route delete` | `DELETE /gateway/routes/:id` |
| `flux schedule create` | `POST /schedules` |
| `flux schedule list` | `GET /schedules` |
| `flux queue create` | `POST /queues` |
| `flux queue publish` | `POST /queues/:name/messages` |
| `flux event publish` | `POST /events` |
| `flux event subscribe` | `POST /events/subscriptions` |
| `flux whoami` | `GET /auth/me` |
| `flux config list` | local file read only |
| `flux config set` | local file write only |
| `flux doctor` | `GET /auth/me`, `GET /health`, `GET /schema/version` |
