# Flux

**Flux records, replays, and resumes backend requests deterministically.**

It lets you debug any request after it happened — with full execution history and exact, safe replay.

Website: [fluxbase.co](https://fluxbase.co)  
Docs: [fluxbase.co/docs](https://fluxbase.co/docs)

---

## The Problem

When a production request fails, debugging usually means opening separate tools: log aggregators, trace UIs, database clients, and deploy history. Reconstructing what actually happened takes time and is error-prone. Reproducing it is risky — live retries can cause duplicate side effects.

Flux solves this by recording everything that happens in a request as a single, replayable unit.

---

## What You Can Do With Flux

```text
Request fails in production
   ↓
flux logs --status error     → find it
flux trace <id>              → see exactly what happened
flux why <id>                → understand the root cause
flux replay <id> --diff      → reproduce it safely, without hitting live systems
flux resume <id>             → continue from the exact point of failure
```

No duplicate emails. No re-running expensive operations. No guessing.

---

## What Flux Guarantees

1. You can replay any recorded request without touching live systems
2. You can inspect exactly what happened — every input, output, and external call
3. You can resume from failure without restarting the whole flow
4. The system never fabricates history

---

## What Flux Gives You

### 1. 📍 `flux trace` — See exactly what happened
Full execution timeline with real data: every step, every call, every response.

### 2. 🤔 `flux why` — Understand why it happened
Root-cause summaries. Not logs — answers. Why did it hit the database? Because there was a cache miss.

### 3. ▶️ `flux replay` — Reproduce it safely
Re-runs the request using recorded data. All external calls return their recorded responses. No side effects re-triggered.

### 4. 🔁 `flux resume` — Continue after failure
Resume from the exact step where it broke. No need to restart from the beginning and risk duplicate effects.

### 5. 📡 `flux tail` — Watch it live
Structured execution traces in real time. Like `tail -f`, but for full request workflows.

---

## How Flux Works

Flux runs your code inside a controlled runtime. Every external call — HTTP requests, database queries, TCP connections — has its result recorded before it is used. This means it can be replayed later without calling the real service.

```text
Your Code (JS/TS)
      ↓
Flux Runtime (V8 Isolate)
      ↓
[ Every external call is recorded ]
      ↓
  ┌───────────┬───────────┬──────────┐
  ↓           ↓           ↓          ↓
fetch()     Postgres    TCP/TLS    (your IO)
  ↓           ↓           ↓          ↓
[ Execution Trace stored in Postgres ]
```

The trade-off is intentional: recording adds overhead to each external call, but it makes every request bulletproof — reproducible and resumable, forever.

---

## Why This Is Different

Most systems rely on retries, locks, and best-effort idempotency.

Flux guarantees:
- deterministic execution
- single durable effects under retries and contention
- replay scoped to recorded history only
- built-in traceability without instrumentation

Flux is not a framework. It is a system that records and controls the execution of backend requests.

---

## What Flux Is Made Of

Flux is three cooperating binaries, all written in Rust:

| Component | Role |
|---|---|
| **`flux` (CLI)** | Developer and operator interface: `logs`, `trace`, `why`, `replay`, `resume`, `exec`, `tail` |
| **`flux-server`** | gRPC server backed by Postgres — stores execution records, traces, and checkpoints |
| **`flux-runtime`** | Deno V8 isolate — runs your JS/TS, records every external call |

All operator commands talk to `flux-server` over gRPC. All state lives in Postgres.

**On Redis:** Redis is optional and fully user-controlled. Flux does not depend on Redis for correctness. If your code uses Redis, Flux records those calls like any other IO.

---

## Compatibility & Roadmap 🧭

Flux targets the **top 20% of libraries** powering **80% of backend applications**.

| Area | Supported | Roadmap |
|------|-----------|---------| 
| **Frameworks** | ✅ Hono | ⚠️ Express, Fastify, Koa |
| **Databases** | ✅ pg (node-postgres) | ⚠️ postgres.js, ioredis |
| **Clients** | ✅ fetch, axios, undici | ✅ Fully Supported |
| **ORMs** | ✅ Drizzle, Kysely | ⚠️ Prisma (limited) |

See the full [**Compatibility Guide**](docs/compatibility.md) and [**Strategic Roadmap**](docs/roadmap.md).

---

## Install

```bash
# macOS / Linux
curl -fsSL https://fluxbase.co/install | bash

# Windows (PowerShell)
irm https://fluxbase.co/install.ps1 | iex
```

## Core Developer Loop

```bash
# start the server
flux server start --database-url postgres://localhost:5432/postgres

# scaffold a project
flux init

# development with reload
flux dev

# build and run
flux build index.ts
flux run index.ts --listen

# send a request
curl -X POST http://localhost:3000/index -d '{"email":"user@example.com"}'

# inspect what happened
flux logs
flux trace <execution_id>
flux why <execution_id>
```

---

## Replay Demo

The shortest proof is the CRUD example in [examples/crud_app](examples/crud_app):

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

This records a real backend request, replays it with the same responses, and suppresses the original Postgres write during replay.

---

## Repository Map

- `cli/` — developer and operator CLI (`flux` binary)
- `server/` — gRPC server + Postgres execution store (`flux-server` binary)
- `runtime/` — Deno V8 isolate executor (`flux-runtime` binary)
- `shared/` — protobuf definitions shared by all three
- `examples/` — sample JS/TS entry files for local testing
- `scripts/` — build, deploy, and test scripts
- `docs/` — product, architecture, and component documentation

---

## Start Here

- [docs/quickstart.md](docs/quickstart.md) — first-run flow
- [docs/concepts.md](docs/concepts.md) — core product model
- [docs/cli.md](docs/cli.md) — command-line workflows
- [docs/compatibility.md](docs/compatibility.md) — supported libraries & tiers
- [docs/production-debugging.md](docs/production-debugging.md) — incident workflow
- [docs/roadmap.md](docs/roadmap.md) — strategic engineering roadmap

---

## Telemetry

The CLI collects anonymous usage events to help us understand how Flux is used. **No personal data, code, or credentials are ever sent** — only CLI version, OS, and arch.

Opt out:
```bash
export FLUX_NO_TELEMETRY=1   # or DO_NOT_TRACK=1
```
