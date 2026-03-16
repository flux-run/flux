# Flux Copilot Instructions

## What Flux Is

Flux is a **backend framework where every execution is a record** — think "Git for backend execution." Every function invocation automatically captures timing spans, database mutations (before/after state), external HTTP calls, and the deployed code SHA, all linked by a single `request_id`. This enables commands like `flux why <id>` (root cause in 10s) and `flux incident replay <id>` (re-run with recorded state).

**Core primitives:** Function · Database · Queue. Everything else composes from these.

**Single source of truth:** All state lives in PostgreSQL. No Redis, no Kafka, no managed queues.

## Repository Structure

Mixed Rust + TypeScript monorepo:

- **Rust workspace** (`Cargo.toml`): `api`, `gateway`, `runtime`, `data-engine`, `queue`, `server` (monolith), `cli`, `shared/job_contract`
- **Node workspaces** (`package.json`): `frontend` (marketing site + docs), `dashboard` (management UI), `packages/functions`, `packages/sdk`, `packages/wasm-sdk`
- **`schemas/`**: Single canonical SQL baseline — `schemas/v0.1.sql` (applied idempotently via `include_str!` in `cli/src/dev.rs`)
- **`docs/`**: Authoritative specs — read `docs/framework.md` (1742 lines) for the full design

## Services & Ports

| Service | Port | Role |
|---------|------|------|
| `api` | 8080 | Management plane — function registry, secrets, schema management |
| `gateway` | 8081 | Public edge — routing, auth, rate limiting, trace root creation |
| `data-engine` | 8082 | DB query engine — mutation recording, hooks, cron |
| `runtime` | 8083 | Serverless executor — Deno V8 isolates run user functions |
| `queue` | 8084 | Async job worker — DB polling, retries, dead-letter queue |
| `server` | 4000 | Monolith — all 5 services in one binary (dev & default) |
| `dashboard` | 5173 | Next.js management UI |

**Dev mode** (`docker-compose.dev.yml`): single `server` binary on :4000 + PostgreSQL on :5432.  
**Production** (`docker-compose.yml`): all five services as separate containers, horizontally scalable (`--scale gateway=4 --scale runtime=8`).

**Architecture direction:** The goal is fully in-process communication (no inter-process HTTP hops between services). The `server` crate is the target monolith. `docs/single-binary-architecture.md` tracks this migration.

## Build & Run Commands

```bash
make dev                        # Start API + dashboard in parallel
make api                        # Run API service (SQLX_OFFLINE=true cargo run)
make dashboard                  # Run dashboard (npm run dev)

make build                      # Build all services
make build SERVICE=api          # Build one service
make build-docker               # Build Docker images for all services
```

## Test Commands

```bash
make test-async-wiring          # Deterministic staging test: Gateway → Queue → Worker → Runtime
make test-platform              # Full platform test suite

# Run a single Rust test:
cd <service> && cargo test <test_name>
cd <service> && cargo test -- --nocapture   # with output

# Run tests for a specific module:
cd api && cargo test route::functions
```

## Database Commands

```bash
make migrate                    # Apply schemas/v0.1.sql to the database
```

SQLx offline mode is enabled for development (`SQLX_OFFLINE=true`). The codebase uses `sqlx::query_as(...)` runtime functions (not macros), so no `.sqlx` cache or `sqlx prepare` step is needed.

## Database Schema Architecture

Three Postgres schemas:
- **`flux.*`** — All Flux system tables (platform internals + queue): `flux.api_*`, `flux.gateway_*`, `flux.runtime_*`, `flux.queue_*`, `flux.jobs`, `flux.job_logs`, `flux.dead_letter_jobs`
- **`flux_internal.*`** — Data-engine introspection tables (table/column metadata, hooks, events, cron, mutations)
- **`public.*`** — User application tables created by `flux db push`

**Ownership rule:** Write to another service's tables only via that service's API endpoint. Exception: the observability tables (`execution_records`, `execution_spans`, `execution_mutations`, `execution_calls`) are append-only and can be written directly by their owning service for hot-path performance.

**Search path:** User functions see only `public.*`. System services see `flux, public` (resolves system tables first).

Mutation recording is **atomic with the data write** — the before/after log is committed in the same transaction. Rolling back the data write rolls back the log.

## Key Architecture Patterns

### All Rust Services Follow Library + Binary Pattern
Every service has `src/lib.rs` + `src/main.rs`. The `server` crate composes all service libraries into the monolith binary.

### Async Jobs Are Database-Backed
The `queue` service polls PostgreSQL (default every 200ms). Job lifecycle: `PENDING → RUNNING → COMPLETED | FAILED → RETRY | DEAD_LETTER`. Visibility timeout: 5 minutes. Max 3 retries with exponential backoff (1s → 2s → ... → 60s). Idempotency keys prevent duplicate execution.

### SQLx for All Database Access
All SQL uses `sqlx::query_as(...)` runtime functions (not macros — no `.sqlx` cache needed). The single schema baseline lives in `schemas/v0.1.sql` and is embedded into the CLI binary via `include_str!`.

### Gateway Routing Uses In-Memory Snapshot
Routes are stored as an in-memory `HashMap<(METHOD, path), function>`, refreshed via Postgres `LISTEN/NOTIFY` for zero-latency updates. All user functions are `POST` endpoints — this is intentional for webhook compatibility.

### Secrets Are Never Logged
Secrets are injected into the Deno V8 isolate at runtime via an LRU cache (30s TTL), encrypted at rest with AES-256-GCM. They never appear in execution records, logs, or error messages.

### Rust Edition Split
- Core services: `edition = "2024"` — `api`, `gateway`, `runtime`, `cli`, `server`
- Supporting crates: `edition = "2021"` — `data-engine`, `queue`, `shared/job_contract`

## User-Facing Function Authoring (TypeScript SDK)

Functions are authored using `@flux/functions` (`packages/functions`), built with `tsup` (ESM + CJS + types), Zod is an optional peer dep.

```typescript
// functions/create_user/index.ts
import { defineFunction } from "@flux/functions";

export default defineFunction({
  input: CreateUserSchema,
  output: UserSchema,
  handler: async (input, ctx) => {
    const user = await ctx.db.users.insert({ ...input });
    await ctx.queue.push("send_welcome_email", { userId: user.id });
    return user;
  },
});
```

**The `ctx` object** is the single interface to all Flux capabilities:

| `ctx.*` | Purpose |
|---------|---------|
| `ctx.db.<table>.<op>()` | Typed DB access (mutations are auto-recorded) |
| `ctx.queue.push(fn, payload, opts)` | Enqueue async job |
| `ctx.function.invoke(name, input)` | Call another function (same `request_id`) |
| `ctx.secrets.get(key)` | Read encrypted secret |
| `ctx.log.info/warn/error()` | Structured log |
| `ctx.error(code, error, message)` | Throw structured error |
| `ctx.requestId` | UUID propagated through entire execution |

**Project layout expected by Flux:**
```
my-app/
├── flux.toml           # project manifest (port, reload, deploy target, limits)
├── functions/          # each subdir = POST /{name} endpoint
├── middleware/         # auth.ts, etc.
├── schemas/            # raw SQL files (source of truth for DB schema)
└── .flux/              # build output, local Postgres data
```

## Observability & Debugging CLI

Every execution record includes: `request_id`, `function_name`, `code_sha`, `input`, `output`, `error`, `duration_ms`, linked spans, mutations, and external calls.

| Command | What it does |
|---------|--------------|
| `flux trace <id>` | Full distributed trace as waterfall |
| `flux why <id>` | Root cause in 10s — error + DB mutations + fix suggestion |
| `flux tail [--errors]` | Live request stream |
| `flux state history <table> --id <row-id>` | Full version history of a row |
| `flux state blame <table>` | Last writer per row |
| `flux incident replay <id>` | Re-run with same input + `code_sha`, externals mocked |
| `flux trace diff <a> <b>` | Compare two executions field-by-field |
| `flux bug bisect --function <fn> --good <sha> --bad <sha>` | Binary-search commits to find regression |

Automatic detections: slow spans (>500ms), N+1 queries (same table ≥3 times), missing indexes, root cause pattern matching (timeouts, constraint violations, permission errors).

## Environment Variables

| Variable | Used By | Purpose |
|----------|---------|---------|
| `DATABASE_URL` | All Rust services | PostgreSQL connection string |
| `INTERNAL_SERVICE_TOKEN` | Gateway, API | Service-to-service auth |
| `LOCAL_MODE` / `FLUX_LOCAL` | Various | Dev mode — disables JWT, tenant routing |
| `PORT` | All services | Service listen port |
| `WORKER_POLL_INTERVAL_MS` | Queue | Job polling interval (default 200ms) |

## Implementation Status

As of March 2026, the project is in **Phase 0** (proving the debugging magic). The Rust infrastructure (CLI, Gateway, Runtime, Data Engine, Queue, API) is built. Still in progress: `flux dev` orchestrator with embedded Postgres, `flux.toml` parser, hot reload, and end-to-end `flux trace` / `flux why` output formatting.

See `docs/implementation-status.md` for the full phase breakdown.

## Deploy Commands

```bash
make deploy                              # Deploy all to production
make deploy SERVICE=api                  # Deploy one service
make deploy-with-migrate SERVICE=api     # Migrate DB first, then deploy
make deploy-gcp                          # Build + push to GCP Artifact Registry + deploy to Cloud Run
```

Always use `deploy-with-migrate` when `schemas/v0.1.sql` has been updated.
