# Flux

> **Public Beta** — install today, replay production bugs tonight.

**Record every request. Replay it. Resume it.**

A request fails in production. Instead of guessing:
- run `flux trace` to see exactly what happened
- run `flux replay` to reproduce it safely
- fix the bug and `flux resume` from the exact failure point

No logs. No guesswork. No retries in production.

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

These guarantees are enforced by the runtime and verified by `examples/compat/flux-contract-suite.ts` — the CI gate that must pass before every release.

---

## Four Core Abilities

| Ability | Command | What it does |
|---|---|---|
| **See what happened** | `flux trace` | Full timeline — every step, every call, every response, real data |
| **Understand why it failed** | `flux why` | Root-cause summaries. Not logs — answers |
| **Re-run it exactly** | `flux replay` | Replays using recorded data. No live systems touched, no side effects re-triggered |
| **Continue after fixing** | `flux resume` | Resumes from the exact step where it broke |
| **Watch it live** | `flux tail` | Structured execution traces in real time |

---

## The Model

Flux separates **truth** from **history**:

- **Truth** (Postgres) converges to one correct, durable state
- **History** (executions) remains complete and honest

Even if:
- a request crashes before recording — no fake history is written
- two requests race — both executions exist, but only one durable result

Correctness does not depend on timing, cache, or execution order.

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
- built-in traceability without any instrumentation

Flux is not a framework.  
Flux is not a runtime replacement.  
Flux is a **control layer over execution**.

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

## Compatibility

> Flux compatibility is defined by whether a library's IO passes through **Flux-controlled boundaries** — not whether it runs. A library that runs but bypasses Flux boundaries silently breaks replay guarantees.

### ✅ Replay-safe (Flux guarantees fully preserved)

| Library | Notes |
|---|---|
| `fetch` (native) | The reference implementation. Zero warnings. |
| `pg` via `flux:pg` | Native Postgres driver with full checkpoint coverage. |
| `drizzle-orm` (over `flux:pg`) | Fully safe ORM layer. |
| `redis` (node-redis) via `flux:redis` | Per-command checkpointing. Blocked commands enforced. |
| `hono` | Pure router. No IO. |
| `jose` | Uses `crypto.subtle` — Flux-controlled. |
| `zod` | Pure computation. No IO. |

### ⚠️ Works with caveats

| Library | Caveat |
|---|---|
| `axios` | Uses `fetch` internally → replay-safe for HTTP calls. Dead browser globals in internals. |
| `ioredis` | Safe when routed through `flux:redis`. Not safe with raw TCP connection. |

### ❌ Breaks execution guarantees (runs, but replay is broken)

| Library | Why | Alternative |
|---|---|---|
| `undici` | Manages its own TCP connection pool — invisible to Flux. Replay fires requests twice. | Use native `fetch` |
| `postgres.js` | Own TCP client — bypasses `flux:pg` interception. | Use `flux:pg` |

See the full [**Compatibility Guide**](docs/compatibility.md).

---

## The Golden Path (Beta)

This is the fully-tested, fully-safe stack for beta users. Everything on this list is replay-safe and covered by the contract test suite.

```ts
import { Hono } from "npm:hono"          // ✅ router
import pg from "flux:pg"                  // ✅ postgres (Flux-native driver)
import { createClient } from "flux:redis" // ✅ redis (Flux-native driver)
import { z } from "npm:zod"              // ✅ validation
import * as jose from "npm:jose"          // ✅ JWT / crypto

// fetch() is available globally — no import needed
// crypto.subtle / crypto.randomUUID() are globally available and deterministic
```

Start with these. Everything else is optional and may have caveats.

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
