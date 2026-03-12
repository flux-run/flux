# Flux Documentation

> **Flux** is a backend framework where every execution is a record.
> Self-hosted, no managed cloud — your infrastructure, your data.

---

## Start here

| Guide | Description |
|---|---|
| [Framework](framework.md) | **The complete spec** — architecture, ctx API, config, phases |
| [Quickstart](quickstart.md) | First function to traced debug in 5 minutes |
| [Core Concepts](concepts.md) | Execution records, ctx, functions, database, flux.toml |
| [CLI Reference](cli.md) | Every `flux` command |
| [Observability](observability.md) | `flux trace`, `flux why`, N+1 detection, debugging |

## Examples

| Example | What it covers |
|---|---|
| [Todo API](examples/todo-api.md) | CRUD with `ctx.db`, schemas, `flux trace` |
| [Webhook Worker](examples/webhook-worker.md) | Verify signature, store event, use `ctx.queue` |
| [AI Backend](examples/ai-backend.md) | OpenAI + `ctx.db` caching + full tracing |

## Service internals (for contributors)

| Service | Port | Role |
|---|---|---|
| [Gateway](gateway.md) | `:8081` | Routing, auth, rate limiting, trace roots |
| [Runtime](runtime.md) | `:8083` | Deno V8 execution, secrets, tool dispatch |
| [API](api.md) | `:8080` | Function registry, logs, schema management |
| [Data Engine](data-engine.md) | `:8082` | DB queries, mutation recording, hooks, cron |
| [Queue](queue.md) | `:8084` | Async jobs, retries, dead letter |

## Additional docs

| Doc | Purpose |
|---|---|
| [Storage](storage.md) | File columns, S3-compatible storage, BYO bucket |
| [Database Schema](database-schema.md) | Table ownership, naming conventions, flux vs public schema |
| [Production Debugging](production-debugging.md) | Deep dive: replay, bisect, trace diff |
| [Git for Backend Execution](git-for-backend-execution.md) | The conceptual model behind Flux |
| [flux why — The Viral Command](flux-why-the-viral-command.md) | Why `flux why` changes how you debug |

---

## Architecture at a glance

```
my-app/
├── flux.toml
├── functions/        → POST endpoints (one directory per function)
├── schemas/          → SQL files (source of truth for DB)
├── middleware/        → request middleware
├── workflows/        → multi-step workflows
└── tests/

$ flux dev → http://localhost:4000

  Gateway     :8081   routing, rate limiting, execution record roots
  Runtime     :8083   Deno V8 execution, secrets, tool dispatch
  API         :8080   function registry, schema management
  Data Engine :8082   DB queries, mutation recording, hooks
  Queue       :8084   async jobs, retries, dead letter

Every request → x-request-id → ExecutionRecord → flux trace / flux why
```

## Quick reference

```bash
flux init my-app && cd my-app   # scaffold project
flux dev                        # start all services + local Postgres
flux invoke hello --data '{}'   # call a function
flux trace <request-id>         # inspect execution record
flux why <request-id>           # root cause in 10 seconds
flux db push                    # apply schemas/*.sql
flux deploy                     # deploy to target from flux.toml
```

---

*The [Framework doc](framework.md) is the single source of truth for all Flux design decisions.*
