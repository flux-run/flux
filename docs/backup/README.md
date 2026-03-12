# Fluxbase Documentation

Fluxbase is a developer platform for serverless backends: functions, database,
secrets, and built-in observability — managed for you, deployed in seconds.

---

## Guides

| Guide | Description |
|---|---|
| [Quickstart](quickstart.md) | Deploy your first function in 5 minutes |
| [Core Concepts](concepts.md) | Functions, database, secrets, gateway |
| [Observability](observability.md) | Tracing, N+1 detection, index suggestions |
| [CLI Reference](cli.md) | Every `flux` command explained |

## Examples

| Example | Description |
|---|---|
| [Todo API](examples/todo-api.md) | Full CRUD API backed by the managed DB |
| [Webhook Worker](examples/webhook-worker.md) | Receive, verify, and process webhooks |
| [AI Backend](examples/ai-backend.md) | OpenAI integration with result caching |

---

## Platform at a glance

```
Your code                  Fluxbase platform
─────────────              ─────────────────────────────────────────
index.ts                   Gateway  ──▶  Runtime (Deno isolate pool)
   flux deploy ──▶               │              │
                                 │        Bundle cache
                                 │          (R2/local)
                                 │
                                 ▼
                           Data Engine
                           ├── Query compiler (plan cache)
                           ├── Complexity guard
                           ├── Edge query cache (gateway layer)
                           └── Postgres (Neon)

Every request:  x-request-id ──▶ spans ──▶ platform_logs
                                           │
                                           └── flux trace <id>
                                               ├── Slow span detection (>500ms)
                                               ├── N+1 query detection (≥3 same table)
                                               ├── Slow DB detection (>50ms)
                                               └── Missing index suggestions
```

---

## Quick reference

```bash
# Authenticate
flux auth login

# Create a project
mkdir my-api && cd my-api && flux init

# Deploy a function
flux deploy .

# Invoke it
flux invoke my_function --data '{"key": "value"}'

# Trace a request end-to-end
flux trace <request-id>

# Tail logs
flux logs my_function --follow

# Manage secrets
flux secrets set MY_KEY my_value
```
