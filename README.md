# Flux

**Flux runs your existing backend inside a deterministic runtime that records every side-effect.**

At its core, Flux is a deterministic execution layer for backend systems. It intercepts and records all I/O (like database queries or external API calls), storing them as a chronological timeline of events. This makes every request 100% debuggable, replayable, and resumable.

Website: [fluxbase.co](https://fluxbase.co)  
Docs: [fluxbase.co/docs](https://fluxbase.co/docs)

## How Flux Works with Your App

Flux sits between your code and the outside world, controlling every interaction with external systems.

```text
Your Code (JS/TS)
      ↓
Flux Runtime (V8 Isolate)
      ↓
[ DETERMINISTIC IO INTERCEPTION ]
      ↓
  ┌───────────┬───────────┬───────────┐
  ↓           ↓           ↓           ↓
fetch()     Postgres    Redis      TCP/TLS
  ↓           ↓           ↓           ↓
[  RECORDED AS EXECUTION TRACE  ]
```

## The Core Mental Model

Every request becomes a recorded execution timeline.

```text
Request
  ↓
Flux Runtime
  ↓
Step 1 → fetch()
Step 2 → postgres.query()
Step 3 → redis.get()
Step 4 → ...
  ↓
```text
Execution Trace (stored)
```

## Compatibility & Roadmap 🧭

Flux aims to support the **top 20% of libraries** that power **80% of backend applications**.

| Area | Supported | Roadmap |
|------|-----------|---------|
| **Frameworks** | ✅ Hono | ⚠️ Express, Fastify, Koa |
| **Databases** | ✅ pg (node-postgres) | ⚠️ postgres.js, ioredis |
| **Clients** | ✅ fetch, axios, undici | ✅ Fully Supported |
| **ORMs** | ✅ Drizzle, Kysely | ⚠️ Prisma (limited) |

### 🚀 Roadmap Highlights
- **Phase 1 (MVP)**: Current focus on Hono + Postgres + Zod.
- **Phase 2 (Ecosystem)**: Full I/O support for `ioredis` and `postgres.js`.
- **Phase 3 (High-Level)**: Background jobs (BullMQ) and deterministic replay of complex state.

See the full [**Compatibility Guide**](docs/compatibility.md) and [**Strategic Roadmap**](docs/roadmap.md) for details.

## What Flux Gives You

### 1. 📍 `flux trace` — What happened?
Shows the full execution timeline with real events, real data, and no guessing.

### 2. 🤔 `flux why` — Why did this happen?
Explains decisions. For example: why did it hit the database? Because there was a cache miss. This provides debugging clarity, not just logs.

### 3. ▶️ `flux replay` — What would happen again?
Re-runs using recorded data. It uses the recorded database response and API responses, ensuring there are no duplicated side effects. This enables deterministic, safe reproduction of bugs.

### 4. 🔁 `flux resume` — Continue after failure
If a request failed at step 3, you can resume execution right from step 3. No need to restart the whole flow and risk duplicate side-effects.

### 5. 📡 `flux tail` — Live execution
Watch your system run structured traces in real time. Like `tail -f`, but for full execution workflows.

## The Real Product Loop

```text
Request fails
   ↓
flux trace   → see what happened
   ↓
flux why     → understand cause
   ↓
flux replay  → reproduce safely
   ↓
flux resume  → continue execution
```

## Why Flux Exists

When a production request fails, debugging usually means opening separate tools: log aggregators, trace UIs, database clients, and deploy history. Reconstructing what actually happened takes time and is error-prone.

Flux records every execution as a single unit:

- the input, output, and error
- every outbound IO call with its request, response, and duration
	- currently includes buffered HTTP fetches, deterministic TCP/TLS exchanges, and native Postgres queries over plain TCP or Rustls-backed TLS
- the total duration and HTTP status

Debugging starts from one execution ID:

```bash
flux logs --status error          # find the failing run
flux trace <id> --verbose         # see the full picture
flux why <id>                     # get a root-cause summary
flux replay <id> --diff           # verify a fix behaves differently
```

## What Flux Includes

- **`flux-runtime`** — executes JS/TS entry files in Deno V8 isolates, records every execution with checkpointed IO calls
- **`flux-server`** — gRPC server backed by Postgres; stores execution records, traces, and checkpoints
- **`flux`** — operator CLI for setup, process management, and incident debugging (`logs`, `trace`, `why`, `replay`, `resume`, `exec`, `tail`)

## Install

```bash
# macOS / Linux
curl -fsSL https://fluxbase.co/install | bash

# Windows (PowerShell)
irm https://fluxbase.co/install.ps1 | iex
```

## Telemetry

The CLI collects anonymous usage events (`flux init`, `flux run`, `flux exec`) to help us understand how Flux is used. **No personal data, code, or credentials are ever sent** — only CLI version, OS, and arch.

Opt out at any time:

```bash
export FLUX_NO_TELEMETRY=1   # or DO_NOT_TRACK=1
```

## Core Developer Loop

The developer loop looks like this:

```bash
# start the server
flux server start --database-url postgres://localhost:5432/postgres

# scaffold a project
flux init

# one-time auth setup
flux init --auth

# development entrypoint with reload
flux dev

# build first; Flux v1 runs bundled artifacts
flux build index.ts
flux run index.ts --listen

# send a request
curl -X POST http://localhost:3000/index -d '{"email":"user@example.com"}'

# inspect what happened
flux logs
flux trace <execution_id>
flux why <execution_id>
```

The experience is:

- one project
- one runtime
- one place to inspect what happened

## Replay Demo

The shortest proof of the operator workflow is the CRUD example in [examples/crud_app](examples/crud_app):

```bash
docker compose -f examples/crud_app/docker-compose.yml up -d postgres
flux server start --database-url postgres://postgres:postgres@localhost:5432/crud_app --service-token dev-service-token

export FLUX_SERVICE_TOKEN=dev-service-token
export DATABASE_URL=postgres://postgres:postgres@localhost:5432/crud_app
export FLOWBASE_ALLOW_LOOPBACK_POSTGRES=1

flux build examples/crud_app/main_flux.ts
flux run --listen --url http://127.0.0.1:50051 --host 127.0.0.1 --port 8000 examples/crud_app/main_flux.ts

curl -i -X POST http://127.0.0.1:8000/todos \
	-H 'content-type: application/json' \
	-d '{"title":"Ship Flux","description":"Replay demo"}'

# copy x-flux-execution-id from the response headers
flux replay <execution_id> --url http://127.0.0.1:50051 --token dev-service-token --diff
```

That flow records a real backend request, replays it with the same response, and suppresses the original Postgres write during replay.

For framework apps and npm dependencies, the intended v1 path is bundled artifacts. See [docs/bundled-artifacts.md](docs/bundled-artifacts.md).

## Product Positioning

Flux feels like:

- a complete backend system
- self-hosted and owned by the team using it
- strongly opinionated about execution records and deterministic debugging

Flux does not feel like:

- a generic cloud control plane
- a bundle of unrelated platform features
- "just another serverless framework"

The headline is debugging. The rest of the system is proof that debugging can stay coherent across the whole backend.

## Why This Is Different

Most systems rely on retries, locks, and best-effort idempotency.

Flux guarantees:

- deterministic execution
- single durable effects under retries and contention
- replay scoped to recorded history
- built-in traceability

Flux is not a framework. Flux is a runtime that controls side effects.

## Architecture At A Glance

Flux is three cooperating binaries:

- `flux` — the CLI (`cli/`)
- `flux-server` — gRPC server and Postgres-backed execution store (`server/` + `shared/`)
- `flux-runtime` — Deno V8 isolate that executes user JS/TS and records checkpoints (`runtime/`)

All operator commands (`flux logs`, `flux trace`, `flux why`, `flux replay`, etc.) talk to `flux-server` over gRPC. `flux-runtime` connects to `flux-server` to validate auth and write execution records. All state is in Postgres.

See [docs/single-binary-architecture.md](docs/single-binary-architecture.md) for the full architecture narrative.

## Repository Map

- `cli/` - developer and operator CLI (`flux` binary)
- `server/` - gRPC server + Postgres execution store (`flux-server` binary)
- `runtime/` - Deno V8 isolate executor (`flux-runtime` binary)
- `shared/` - protobuf definitions shared by CLI, server, and runtime
- `examples/` - sample JS entry files for local testing
- `scripts/` - build, deploy, and test scripts
- `docs/` - product, architecture, and component documentation

## Start Here

- [docs/README.md](docs/README.md) - documentation map
- [docs/quickstart.md](docs/quickstart.md) - first-run flow
- [docs/compatibility.md](docs/compatibility.md) - supported libraries & tiers
- [docs/roadmap.md](docs/roadmap.md) - strategic engineering roadmap
- [docs/concepts.md](docs/concepts.md) - core product model
- [docs/cli.md](docs/cli.md) - command-line workflows
- [docs/production-debugging.md](docs/production-debugging.md) - incident workflow
- [docs/SPEC.md](docs/SPEC.md) - product goals and design constraints

## Open Source Direction

Flux is an open-source backend runtime for teams that want:

- full control over runtime and data
- a simpler local-to-production mental model
- much stronger operational debugging than logs-first stacks provide

The project is understandable from the repo itself. The docs explain the product, the architecture, and the workflows that make Flux valuable.
