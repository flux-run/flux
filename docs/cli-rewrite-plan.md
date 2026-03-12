# CLI Rewrite Plan

**Date:** March 12, 2026  
**Context:** Gateway, Runtime, Queue, and Data Engine have been rewritten with clean
SOLID architecture. This document audits every existing `flux` CLI command and
produces a rewrite plan aligned with the framework (self-hosted, single-project,
developer-first).

---

## What the rewrite must solve

The current CLI was built for a **cloud-hosted multi-tenant product**. It talks to
a remote Fluxbase API and wraps `docker compose`. The rewritten CLI must be a
**local framework CLI** — it orchestrates services on the developer's machine,
manages project structure, and is the primary interface for observability/debugging.

Key shifts:

| Current (cloud) | Rewritten (framework) |
|---|---|
| `--tenant`, `--project` global flags | Context is the current directory (`flux.toml`) |
| `flux tenant list/create/delete` | Irrelevant — removed |
| `flux project list/create/delete` | Irrelevant — removed |
| `flux deploy` pushes to remote API | `flux deploy` hot-swaps into local `flux dev` or builds Docker image |
| `flux dev` wraps `docker compose up` | `flux dev` spawns native binaries (no Docker required) |
| Observability reads from remote API | Observability reads from local Postgres |
| `flux stack`/`flux server` overlap with `flux dev` | One command: `flux dev` |

---

## Command inventory and status

Legend: ✅ Keep (works today, minimal changes) · 🔧 Rewrite (exists, wrong model) · 🆕 Build (needs to exist) · ❌ Drop (cloud/tenant-specific, irrelevant to framework)

---

### Auth

| Command | Status | Notes |
|---|---|---|
| `flux login` | ❌ Drop | No managed cloud in framework. Remove. |
| `flux whoami` | ❌ Drop | Same. |

---

### Project scaffolding

| Command | Status | Notes |
|---|---|---|
| `flux init [--name] [--runtime]` | 🔧 Rewrite | Keep structure, drop API call, write `flux.toml` only |
| `flux new <name> [--template]` | 🔧 Rewrite | Scaffold from local templates; drop remote template fetch |
| `flux dev` | 🔧 Rewrite | **Critical.** Currently wraps `docker compose`. Must spawn native service binaries, watch `functions/`, hot-reload. Full rewrite. |

---

### Functions

| Command | Status | Notes |
|---|---|---|
| `flux function create <name>` | ✅ Keep | Scaffold `functions/<name>/index.ts` + `flux.json` |
| `flux function list` | 🔧 Rewrite | Read from local API service instead of remote |
| `flux function delete <name>` | 🔧 Rewrite | Same |
| `flux invoke <name> [--payload <json>]` | 🔧 Rewrite | Point at local gateway (`localhost:4000`) not remote |
| `flux build [name]` | 🆕 Build | **Does not exist.** Bundle TS→JS via esbuild, write `.flux/build/<name>/` |
| `flux deploy [--name] [--target local|docker|k8s]` | 🔧 Rewrite | Currently pushes to remote. Must support `--target local` (hot-swap into `flux dev`) and `--target docker` (build image) |

---

### Deployment versions

| Command | Status | Notes |
|---|---|---|
| `flux version list` | ❌ Drop | Cloud-hosted versioning. Framework handles this via git SHA in execution records. |
| `flux version rollback <v>` | ❌ Drop | Same. |
| `flux version promote` | ❌ Drop | Same. |
| `flux version diff` | ❌ Drop | Same. |
| `flux deployments list` | ❌ Drop | Same. |

---

### Local dev stack

| Command | Status | Notes |
|---|---|---|
| `flux dev` | 🔧 Rewrite | See above — the core command. |
| `flux server [--port] [--only] [--release]` | ❌ Drop | Overlaps with `flux dev`. Remove. One entry point. |
| `flux stack up` | ❌ Drop | Wrapper for `docker compose up`. Irrelevant once `flux dev` is native. |
| `flux stack down` | ❌ Drop | Same. |
| `flux stack ps` | ❌ Drop | Same. |
| `flux stack logs` | ❌ Drop | Replaced by `flux logs`. |
| `flux stack reset` | ❌ Drop | Replaced by `flux db reset`. |
| `flux stack seed` | ❌ Drop | Replaced by `flux db seed`. |

---

### Database

| Command | Status | Notes |
|---|---|---|
| `flux db push` | 🆕 Build | Apply `schemas/*.sql` to local Postgres. **Does not exist.** |
| `flux db diff` | 🆕 Build | Compare `schemas/*.sql` vs `information_schema`. **Does not exist.** |
| `flux db migrate` | 🆕 Build | Save diff as timestamped `.sql` migration file. **Does not exist.** |
| `flux db seed` | 🆕 Build | Apply `tests/fixtures/*.sql`. **Does not exist.** |
| `flux db reset` | 🆕 Build | Drop + recreate + push + seed. **Does not exist.** |
| `flux db query --sql <sql>` | ✅ Keep | Run raw SQL against local DB |
| `flux db shell` | ✅ Keep | Open `psql` shell |
| `flux db create` | ❌ Drop | Cloud DB provisioning. Irrelevant. |
| `flux db list` | ❌ Drop | Same. |
| `flux db table` (subcommands) | ❌ Drop | Managed DB metadata. Irrelevant. |
| `flux db diff --env1 --env2` | 🔧 Rewrite | Change from env-to-env comparison to schema-vs-live comparison |
| `flux db history <table> --id` | ✅ Keep | Reads `state_mutations` — works already |

---

### Secrets

| Command | Status | Notes |
|---|---|---|
| `flux secrets set <key> <value>` | 🔧 Rewrite | Currently talks to remote API. Should write to `.env.local` for dev, API for deployed. |
| `flux secrets get <key>` | 🔧 Rewrite | Same. |
| `flux secrets list` | 🔧 Rewrite | Same. |
| `flux secrets delete <key>` | 🔧 Rewrite | Same. |

---

### Observability & Debugging

This is the core value of the framework. All infrastructure exists in Rust.
All CLI commands need to read from **local Postgres** (not remote API).

| Command | Status | Notes |
|---|---|---|
| `flux trace [<id>]` | 🔧 Rewrite | Infrastructure: ✅ `trace_requests` + `platform_logs`. Rewrite to remove tenant/project auth, point at local DB. |
| `flux trace <id> --flame` | 🔧 Rewrite | Same. |
| `flux trace diff <a> <b>` | 🔧 Rewrite | Same. |
| `flux trace debug <id> [--interactive]` | 🔧 Rewrite | Same. |
| `flux why <id>` | 🔧 Rewrite | Infrastructure: ✅ `platform_logs` + `state_mutations`. Same rewrite. |
| `flux debug [<id>]` | 🔧 Rewrite | Same. |
| `flux fix [<id>]` | ✅ Keep | Alias for `flux debug`. Keep. |
| `flux tail [function] [--errors] [--slow]` | 🔧 Rewrite | Infrastructure: ✅. Remove tenant auth. |
| `flux logs [source] [--follow]` | 🔧 Rewrite | Same. |
| `flux errors [--function] [--since]` | 🔧 Rewrite | Same. |
| `flux state history <table> --id <id>` | 🔧 Rewrite | Infrastructure: ✅ `state_mutations`. Remove tenant/project. |
| `flux state blame <table>` | 🔧 Rewrite | Same. |
| `flux incident replay <id>` | 🔧 Rewrite | Infrastructure: ✅. Remove tenant/project. |
| `flux bug bisect --function --good --bad` | 🔧 Rewrite | Infrastructure: ✅. Remove tenant/project. |
| `flux explain [file]` | ✅ Keep | Data Engine query explainer. Works already. |
| `flux monitor` | ❌ Drop | Cloud monitoring / alerting. Out of scope. |

---

### Config

| Command | Status | Notes |
|---|---|---|
| `flux config get/set/list` | 🔧 Rewrite | Keep structure. Config lives in `~/.flux/config.json` and local `flux.toml`. Remove remote-project config. |

---

### API Keys

| Command | Status | Notes |
|---|---|---|
| `flux api-key create/list/revoke/rotate` | ❌ Drop | Cloud API key management. Framework uses secrets instead. |

---

### Gateway

| Command | Status | Notes |
|---|---|---|
| `flux gateway route create/delete/activate` | ❌ Drop | Routes are derived from `functions/` automatically — `flux deploy` owns create/delete. |
| `flux gateway route list` | 🆕 Build | Show all live routes + their operational config. |
| `flux gateway route patch <path>` | 🆕 Build | Mutate operational metadata (auth, rate_limit, cors, json_schema) on an existing route. |

---

### Workflows

| Command | Status | Notes |
|---|---|---|
| `flux workflow create` | 🔧 Rewrite | Keep. Scaffold `workflows/<name>.ts`. Remove remote API call. |
| `flux workflow deploy` | 🔧 Rewrite | Upload workflow definition to local API. |
| `flux workflow run` | 🔧 Rewrite | Trigger via local gateway. |
| `flux workflow logs` | 🔧 Rewrite | Read from local `platform_logs`. |
| `flux workflow trace` | 🔧 Rewrite | Read from local trace tables. |
| `flux workflow list` | 🔧 Rewrite | Read from local API. |

---

### Agents

| Command | Status | Notes |
|---|---|---|
| `flux agent create/deploy/run/simulate` | 🔧 Rewrite | Phase 3+. Keep structure, remove remote calls. |

---

### Cron

| Command | Status | Notes |
|---|---|---|
| `flux cron list` | 🔧 Rewrite | Read from local Data Engine cron state. Renamed from `flux schedule list`. |
| `flux cron pause/resume` | 🔧 Rewrite | Manage via local API. |
| `flux cron history` | 🔧 Rewrite | Read from local logs. |

---

### Queue

| Command | Status | Notes |
|---|---|---|
| `flux queue list` | 🔧 Rewrite | Read from local Postgres queue tables. |
| `flux queue retry <id>` | 🔧 Rewrite | Same. |
| `flux queue dead-letter` | 🔧 Rewrite | Same. |
| `flux queue create/publish` | ❌ Drop | Cloud queue provisioning. Use `ctx.queue.push()` instead. |

---

### Events

| Command | Status | Notes |
|---|---|---|
| `flux event list` | 🔧 Rewrite | List registered event types from local Data Engine. |
| `flux event publish <type>` | 🔧 Rewrite | Publish an event manually (useful for testing triggers). |
| `flux event history <type>` | 🔧 Rewrite | Show recent events of this type from local tables. |
| `flux event subscribe` | ❌ Drop | Programmatic concept — belongs in `ctx.event.on()`, not CLI. |

---

### Tools

| Command | Status | Notes |
|---|---|---|
| `flux tool list` | ✅ Keep | Lists available integrations. |
| `flux tool connect/disconnect` | 🔧 Rewrite | Store secrets locally instead of remote. |
| `flux tool run` | ✅ Keep | Works already. |

---

### Environments

| Command | Status | Notes |
|---|---|---|
| `flux env create/delete/clone` | ❌ Drop | Cloud environment management. Framework uses `--target` flag on `flux deploy` instead. |

---

### SDK (Type Generation)

| Command | Status | Notes |
|---|---|---|
| `flux pull [--output]` | 🔧 Rewrite | Was: download SDK from remote. Rewrite as: `flux generate` — read `information_schema` from local DB, emit `flux.d.ts`. |
| `flux watch [--output]` | 🔧 Rewrite | Auto-regenerate on schema change. Keep concept, point at local DB. |
| `flux status [--sdk]` | ❌ Drop | Remote schema version status. Irrelevant. |

---

### Utilities

| Command | Status | Notes |
|---|---|---|
| `flux doctor [<request-id>]` | ✅ Keep | Diagnose environment/connectivity. Already works locally. |
| `flux open` | ❌ Drop | Opens Fluxbase cloud dashboard. Framework has local trace viewer at `localhost:4000/trace/<id>`. |
| `flux upgrade [--check]` | ✅ Keep | Self-update via GitHub Releases. |

---

### Tenant / Project (top-level management)

| Command | Status | Notes |
|---|---|---|
| `flux tenant create/list/delete` | ❌ Drop | Multi-tenant cloud concept. Irrelevant. |
| `flux project create/list/delete` | ❌ Drop | Same. |

---

## New CLI — complete command reference

Global flags available on every command:

```
--json          Output machine-readable JSON
--no-color      Disable coloured output
--quiet         Suppress non-error output
--verbose       Detailed/debug output
--dry-run       Show what would happen without making changes
--yes           Skip confirmation prompts
--dir <path>    Project root (default: cwd, walks up to find flux.toml)
```

---

### Project

```
flux init
  [name]                    Project name (written to flux.toml)
  --runtime <id>            nodejs20 | bun | deno  (default: nodejs20)
  --port <n>                Gateway port  (default: 4000)

  Creates flux.toml + functions/ + schemas/ + tests/ in the current directory.
  Writes sensible defaults. Safe to re-run (no-op if file exists).

flux new <name>
  --template <id>           blank | todo-api | ai-backend | webhook-worker
                            (default: blank)

  Scaffold a full project from a template into a new <name>/ directory.
  Equivalent to: mkdir <name> && cd <name> && flux init.

flux dev
  --clean                   Wipe .flux/pgdata/ and start fresh
  --port <n>                Override gateway port from flux.toml

  Start all services (Gateway, Runtime, API, Data Engine, Queue) plus a managed
  local Postgres. No Docker required. Watches functions/ for changes and
  hot-reloads on save (<200ms). Prints a single URL to hit.

  Service ports:
    Gateway      :4000  (or --port)
    API          :8080
    Data Engine  :8082
    Runtime      :8083
    Queue        :8084
    Postgres     :5432  (auto-assigned, written to .flux/dev.env)
```

---

### Functions

```
flux function create <name>
  --description <text>      Written into the generated flux.json
  --middleware <group>      Middleware group from flux.toml (default: none)

  Scaffold functions/<name>/index.ts and functions/<name>/flux.json.

flux function list
  --json                    Raw JSON array

  List all functions in the project with their last-deployed status.

flux function delete <name>
  --yes                     Skip confirmation

  Remove the function from the registry and delete functions/<name>/.

flux build
  [name]                    Build a single function (default: all)
  --watch                   Rebuild on file change

  Bundle TypeScript → single JS via esbuild.
  Output: .flux/build/<name>/function.js + metadata.json
  metadata.json contains: name, git_sha, built_at, input_schema, output_schema.

flux deploy
  [name]                    Deploy a single function (default: all changed)
  --target <t>              local | docker | k8s  (default: local)
  --build                   Run flux build first

  --target local:   POST /internal/functions to local API + invalidate runtime cache.
                    Zero-downtime hot-swap into running flux dev.
  --target docker:  Build a FROM flux/runtime Docker image with artifacts baked in.
  --target k8s:     Write Kubernetes manifests to .flux/k8s/.

flux invoke <name>
  --data <json>             Input payload as JSON string
  --file <path>             Read payload from a JSON file (use - for stdin)
  --pretty                  Pretty-print response (default when TTY)

  Call a function through the local gateway (localhost:4000/<name>).
```

---

### Database

```
flux db push
  --dry-run                 Print SQL without executing (same as flux db diff)

  Apply schemas/*.sql to the local Postgres. Safe to run repeatedly — only
  executes the diff, never drops existing data.

flux db diff
  Print the SQL that flux db push would run. Never executes anything.

flux db migrate
  --name <label>            Filename suffix (e.g. "add_orders_table")

  Compute the diff, save it as migrations/<timestamp>_<name>.sql, print the path.
  Does NOT apply the migration.

flux db seed
  --file <path>             Specific fixture file (default: tests/fixtures/*.sql)
  --reset                   Run flux db reset first

  Execute fixture SQL files against the local database.

flux db reset
  --yes                     Skip confirmation

  Drop + recreate + flux db push + flux db seed. Destructive. Requires --yes or
  interactive confirmation.

flux db query
  --sql <sql>               SQL string to execute
  --file <path>             Path to a .sql file (use - for stdin)
  --db <name>               Database name (default: "default")

  Run arbitrary SQL and print results as a table (or --json for array).

flux db shell
  --db <name>               Database name (default: "default")

  Open an interactive psql session against the local Postgres.

flux db history <table>
  --id <value>              Primary key value to filter to
  --limit <n>               Number of rows (default: 50)
  --json                    Raw JSON

  Show the full before/after mutation history for a table (or single row).
  Reads from state_mutations table.
```

---

### Secrets

```
flux secrets set <key> <value>
  Persist a secret. In dev: written to .env.local (gitignored).
  .env.local is auto-loaded by flux dev and injected into all services.

flux secrets get <key>
  Print the value of a secret (cleartext).

flux secrets list
  List all keys. Values are always redacted.

flux secrets delete <key>
  --yes                     Skip confirmation
  Remove a secret.
```

---

### Observability & Debugging

```
flux trace
  [request-id]              Show full trace for one request
  --limit <n>               Number of recent traces to list (default: 20)
  --function <name>         Filter list to a function
  --slow <ms>               Filter list to requests slower than this (default: 500)
  --flame                   Render a Gantt-style waterfall after the span tree
  --json                    Raw JSON

  No request-id: list recent traces (most recent first).
  With request-id: render full span tree with timing, db mutations, external calls.

flux trace diff <id-a> <id-b>
  --table <name>            Limit mutation diff to one table
  --json                    Raw JSON

  Compare two executions field-by-field: status, duration, mutations, spans.
  Typical use: original production run vs a replay.

flux trace debug <id>
  --at <step>               Jump directly to step N (1-based)
  --interactive             Step through spans with Enter / s / p / q
  --json                    Raw JSON

  Step-through debugger: walks the execution graph span-by-span, showing
  which DB mutations happened at each step.

flux why <request-id>
  --json                    Raw JSON

  Root cause in 10 seconds. Parses spans + error + mutations and prints:
  - What failed and where
  - Which DB rows were touched
  - Suggested next command (flux trace debug, flux state history, etc.)

flux debug
  [request-id]              Deep-dive a specific request
  --replay                  Re-run the request after inspecting
  --no-logs                 Skip log section
  --json                    Raw JSON

  No request-id: interactive mode — lists recent errors, pick one to inspect.
  With request-id: full deep-dive (spans + logs + mutations + external calls).

flux fix [request-id]       Alias for flux debug. Shorter to type in an alert.

flux tail
  [function]                Filter to a single function name
  --errors                  Show only failed requests
  --slow <ms>               Show only requests slower than N ms
  --json                    One JSON object per line
  --auto-debug              Pause and run flux debug when an error appears

  Live request stream. Shows: method, route, function, duration, status.
  Errors print a "flux debug <id>" hint inline.

flux logs
  [source]                  function | db | workflow | event | queue | system
  [resource]                Function name or workflow name to filter
  --follow                  Stream new lines as they arrive (like tail -f)
  --limit <n>               Lines to show (default: 100)

  Tail service and function logs. "flux logs" with no args shows all sources.
  "flux logs create_user --follow" tails a single function.

flux errors
  --function <name>         Filter to a specific function
  --since <duration>        Time window: 1h, 24h, 7d  (default: 1h)
  --json                    Raw JSON

  Per-function error summary: count, most recent error code, p50/p95 duration.
  Good first stop before flux debug.

flux state history <table>
  --id <value>              Filter to a single row by primary key
  --limit <n>               Number of mutations (default: 50)
  --json                    Raw JSON

  Full before/after version history for every mutation to a table (or row).
  Shows: operation, timestamp, writer (request_id), field diffs.

flux state blame <table>
  --json                    Raw JSON

  Last writer per row: who last touched each row and from which request.

flux incident replay <request-id>
  --write                   Allow DB writes (default: suppressed)
  --live-http               Make real outbound HTTP calls (default: mocked)
  --yes                     Skip confirmation

  Re-execute with the exact same input and code SHA.
  DB reads are live. Writes, queue pushes, and HTTP calls are mocked by default.
  Creates a new execution record tagged replay:true pointing at the original.

flux bug bisect
  --function <name>         Function to analyse  [required]
  --good <sha>              Known-good commit SHA (prefix ok)  [required]
  --bad <sha>               Known-bad commit SHA (prefix ok)   [required]
  --threshold <0.0-1.0>     Error-rate threshold to classify a commit as bad
                            (default: 0.05)
  --json                    Raw JSON

  Binary-search trace history to find the first commit where error rate crossed
  the threshold. Reads recorded execution history — no replays needed.

flux explain
  [file]                    Path to query JSON file (use - for stdin)
  --json                    Raw JSON

  Dry-run a Data Engine query: show compiler output, applied policies,
  complexity score, and final SQL without executing against the database.
```

---

### Queue

```
flux queue list
  --status <s>              pending | running | failed | dead-letter
  --function <name>         Filter by handler function
  --limit <n>               (default: 50)
  --json                    Raw JSON

  List jobs in the queue. Shows: id, function, status, attempts, next_run_at.

flux queue retry <job-id>
  --yes                     Skip confirmation

  Re-enqueue a failed job immediately, resetting its retry counter.

flux queue dead-letter
  --limit <n>               (default: 50)
  --json                    Raw JSON

  List jobs that have exhausted all retries and landed in the dead-letter queue.
  Use flux queue retry <id> to re-attempt one.
```

---

### Cron

```
flux cron list
  --json                    Raw JSON

  List all cron jobs registered in the project, their schedule expression,
  last run time, next run time, and status (active/paused).

flux cron pause <name>
  Pause a cron job without deleting it.

flux cron resume <name>
  Resume a paused cron job.

flux cron history <name>
  --limit <n>               (default: 20)
  --json                    Raw JSON

  Show recent invocation history for a cron job: run time, duration, status.
  Each row links to a request-id usable with flux trace / flux why.
```

---

### Workflows

```
flux workflow create <name>
  Scaffold workflows/<name>.ts with a defineWorkflow() template.

flux workflow list
  --json                    Raw JSON
  List all workflow definitions + their last deployment status.

flux workflow deploy <name>
  Upload the workflow definition to the local API service.

flux workflow run <name>
  --data <json>             Input payload
  --file <path>             Read payload from file

  Trigger a workflow and stream its step output until complete.

flux workflow list-runs
  --workflow <name>         Filter to a workflow
  --status <s>              running | completed | failed
  --limit <n>               (default: 20)
  --json                    Raw JSON

  List active and recent workflow runs.

flux workflow trace <run-id>
  --json                    Raw JSON

  Show the full execution trace for a workflow run: steps, timing, mutations.
```

---

### Code Generation

```
flux generate
  --output <path>           Output file (default: flux.d.ts in project root)
  --watch                   Re-run whenever the DB schema changes

  Read information_schema from the local Postgres (via GET /internal/introspect)
  and emit TypeScript types:
    - ctx.db.<table> typed accessors
    - ctx.function.invoke() overloads
    - Secret key literals
    - Tool action types
```

---

### Gateway

```
flux gateway route list
  --json                    Raw JSON

  Show all live routes, their target function, and operational config:
  auth_type, rate_limit, cors_origins, json_schema, active status.

flux gateway route patch <path>
  --auth-type <type>        none | api_key | jwt
  --rate-limit <n>          Requests per minute (0 = unlimited)
  --cors-origins <origins>  Comma-separated allowed origins
  --json-schema <file>      Path to a JSON Schema file for request validation

  Mutate operational metadata on an existing route without redeploying.
  Route must already exist (created by flux deploy).
  Changes take effect immediately via Postgres NOTIFY to the gateway.
```

---

### Events

```
flux event list
  --json                    Raw JSON

  List all registered event types in the project.

flux event publish <type>
  --data <json>             Event payload
  --file <path>             Read payload from file

  Publish an event manually. Useful for testing event-triggered functions
  and workflows without writing a producer function.

flux event history <type>
  --limit <n>               (default: 20)
  --json                    Raw JSON

  Show recent events of this type: timestamp, payload summary, triggered functions.
  Each row links to a request-id usable with flux trace / flux why.
```

---

### Tools

```
flux tool list
  --installed               Show only connected tools
  --json                    Raw JSON

  List available tool integrations (Stripe, OpenAI, Resend, etc.).

flux tool connect <name>
  Walk through required secrets for the integration and save them to .env.local.
  Runs flux generate after connecting to update ctx.tools types.

flux tool disconnect <name>
  --yes                     Skip confirmation
  Remove the tool's secrets and update types.

flux tool run <name> <action>
  --data <json>             Input for the action
  --file <path>             Read input from file

  Run a single tool action directly. Useful for testing integrations.
```

---

### Config

```
flux config list
  Print all active configuration: flux.toml values + ~/.flux/config.json values.
  Shows effective value and which file it came from.

flux config get <key>
  Print the value of a single config key.

flux config set <key> <value>
  Persist a value to ~/.flux/config.json (global) or flux.toml (project).
  --global                  Write to ~/.flux/config.json
```

---

### Utilities

```
flux doctor
  [request-id]              Diagnose a specific failed request (omit for env check)

  No request-id: check environment — binary versions, Postgres connectivity,
  service health, port conflicts.
  With request-id: deep-dive that request and print a diagnosis report.

flux upgrade
  --check                   Print latest version without upgrading
  --version <v>             Install a specific version

  Self-update the flux binary via GitHub Releases.
```

---

## Rewrite phases

### Phase 0 (unblock the demo) — ~2 weeks

These 5 commands are the critical path. Nothing else matters until they work end-to-end.

| Command | Work | Blocker for |
|---|---|---|
| `flux init` | Strip API call, write `flux.toml` only | Every other command |
| `flux dev` | Spawn native service binaries, health-check loop, Ctrl+C cleanup | All local development |
| `flux build <name>` | Shell out to `esbuild` (or call `@fluxbase/bundler`), write `.flux/build/<name>/metadata.json` | `flux deploy`, `flux invoke` |
| `flux deploy --target local` | `POST /internal/functions` to local API, invalidate runtime cache | `flux invoke` |
| `flux trace` / `flux why` | Remove tenant auth header, point at `localhost:8080` | The debug value prop |

---

### Phase 1 (golden path works) — ~2 weeks

| Command | Work |
|---|---|
| `flux function create/list/delete` | Point at local API |
| `flux invoke` | Point at `localhost:4000` (local gateway) |
| Hot reload in `flux dev` | FSEvents on `functions/` → `flux build` → `flux deploy --target local` |
| `flux secrets` | Write/read `.env.local`; inject into dev stack via env |
| `flux tail` / `flux logs` / `flux errors` | Remove tenant auth, point at local API |

---

### Phase 2 (database workflow) — ~2 weeks

| Command | Work |
|---|---|
| `flux db push` | Parse `schemas/*.sql`, diff vs `information_schema`, apply |
| `flux db diff` | Diff only, print colored SQL |
| `flux db migrate` | Write timestamped file to `migrations/` |
| `flux db seed` | Execute `tests/fixtures/*.sql` |
| `flux db reset` | Drop + recreate + push + seed |
| `flux generate` | Call `GET /internal/introspect`, write `flux.d.ts` |

---

### Phase 3 (production ops) — ~2 weeks

| Command | Work |
|---|---|
| `flux deploy --target docker` | Build `FROM flux/runtime` image |
| `flux deploy --target k8s` | Write manifests to `.flux/k8s/` |
| `flux queue` subcommands | Wire to local queue Postgres tables |
| `flux cron list` | Wire to local Data Engine cron state |
| `flux incident replay` | Replay via runtime with side-effect suppression |
| `flux bug bisect` | Read local trace history, binary search by commit SHA |

---

### Phase 4 (remaining) — ongoing

| Command | Work |
|---|---|
| `flux workflow create/run/trace` | Wire to Data Engine workflow engine |
| `flux agent` | Wire to AI agent runtime |
| `flux tool connect/disconnect` | Local tool registry |
| `flux state history/blame` | Already working — just strip tenant auth |
| `flux trace diff/debug` | Already working — same strip |
| `flux event list/publish/history` | Wire to Data Engine event tables |
| `flux gateway route list/patch` | Wire to gateway routes table; patch sends NOTIFY |

---

## Global flag cleanup

Current global flags that should be **removed** in the rewrite:

```
--tenant   (cloud multi-tenancy — gone)
--project  (cloud project selection — gone; project = current directory)
--env      (cloud environment targeting — gone; environment = local or deploy target)
```

Global flags to **keep or add**:

```
--json        Machine-readable output
--no-color    Disable color
--quiet       Suppress non-error output
--verbose     Detailed output
--dry-run     Preview without executing
--yes         Skip confirmation prompts
--dir <path>  Project root (default: cwd + flux.toml search)
```

---

## Client architecture change

Current: `client.rs` builds `Authorization: Bearer <token>` headers from `~/.flux/config.json` credentials and sends everything to `https://api.fluxbase.io`.

Rewritten: `client.rs` reads the `[dev]` section of `flux.toml` for port config
and sends to `http://localhost:<port>`. No auth headers needed — local services
accept all traffic.

```rust
// Current
pub fn api_base() -> String {
    "https://api.fluxbase.io/v1".to_string()
}

// Rewritten
pub fn api_base() -> String {
    let port = FluxToml::load_sync()
        .and_then(|t| t.dev.api_port)
        .unwrap_or(8080);
    format!("http://localhost:{port}")
}
```

---

## Cargo.toml dependency cleanup

Commands being dropped will allow removing some deps:

| Dep | Reason to keep/remove |
|---|---|
| `reqwest` | Keep — still calls local services |
| `clap` | Keep |
| `tokio` | Keep |
| `serde_json` | Keep |
| `colored` | Keep |
| `which` | Keep — used in `flux dev` to check for docker/binaries |
| `dialoguer` | Keep — used in interactive `flux debug` |
| `indicatif` | Keep — progress bars |
| `keyring` | Remove — was for storing cloud credentials |
| `open` (crate) | Remove — only used by `flux open` (cloud dashboard) |

---

*Build the demo first. Phase 0 is the only thing that matters right now.*
