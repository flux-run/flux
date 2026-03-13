# API Reference — Flux Server

**Base URL:** `http://localhost:4000/flux/api`  
**Auth:** `Authorization: Bearer <FLUX_API_KEY>` — required when `FLUX_API_KEY` is set on the server.  
In dev mode (no key set) all requests pass through unauthenticated.

**Status legend**

| Symbol | Meaning |
|--------|---------|
| ✅ | Fully implemented with real database queries |
| 🟡 | Stub — returns empty list / 501 Not Implemented; placeholder until built |
| 🔒 | Internal — service-token only, not for CLI/dashboard use |

---

## Utility (no auth required)

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/health` | ✅ | `{"status":"ok"}` |
| `GET` | `/version` | ✅ | `service`, `commit`, `build_time` |
| `GET` | `/openapi.json` | ✅ | OpenAPI 3.x spec |
| `GET` | `/openapi/ui` | ✅ | Swagger UI |

---

## Internal (service-token `X-Service-Token` — 🔒)

All paths below `/internal/`. Not exposed to CLI or dashboard users.

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/internal/secrets` | 🔒 ✅ | Runtime secret injection |
| `GET` | `/internal/bundle` | 🔒 ✅ | Download function bundle JS |
| `GET` | `/internal/introspect` | 🔒 ✅ | DB schema + function + agent contracts for `flux generate` |
| `POST` | `/internal/logs` | 🔒 ✅ | Runtime writes execution logs |
| `GET` | `/internal/logs` | 🔒 ✅ | Service log reads |
| `GET` | `/internal/functions/resolve` | 🔒 ✅ | Gateway route → function lookup |
| `POST` | `/internal/cache/invalidate` | 🔒 ✅ | Hot-reload: flush function cache |

---

## Functions

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/functions` | ✅ | List all functions |
| `POST` | `/functions` | ✅ | Register a function |
| `GET` | `/functions/{id}` | ✅ | Get function metadata |
| `DELETE` | `/functions/{id}` | ✅ | Remove function |
| `GET` | `/functions/resolve?name=` | 🔒 ✅ | Internal: resolve by name |

---

## Deployments

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `POST` | `/functions/deploy` | ✅ | Upload + deploy bundle (10 MB limit) |
| `POST` | `/deployments` | ✅ | Create deployment record |
| `GET` | `/deployments/list/{function_id}` | ✅ | List deployments for a function |
| `POST` | `/deployments/{id}/activate/{version}` | ✅ | Promote a version to active |

---

## Secrets

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/secrets` | ✅ | List secret keys (values redacted) |
| `POST` | `/secrets` | ✅ | Create secret |
| `PUT` | `/secrets/{key}` | ✅ | Update secret value |
| `DELETE` | `/secrets/{key}` | ✅ | Delete secret |

---

## Logs & Traces

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/logs` | ✅ | List project logs (paged) |
| `GET` | `/traces` | ✅ | List recent execution traces |
| `GET` | `/traces/{request_id}` | ✅ | Full execution record: spans, mutations, calls |

---

## Gateway Routes

Manage function routing rules. Changes take effect immediately via Postgres NOTIFY.

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/gateway/routes` | ✅ | List all routes |
| `POST` | `/gateway/routes` | ✅ | Create route |
| `GET` | `/gateway/routes/{id}` | 🟡 | Get single route |
| `PATCH` | `/gateway/routes/{id}` | ✅ | Update route config |
| `DELETE` | `/gateway/routes/{id}` | ✅ | Delete route |
| `POST` | `/gateway/middleware` | 🟡 | Attach middleware to a route |
| `DELETE` | `/gateway/middleware/{route}/{type}` | 🟡 | Remove middleware |
| `PUT` | `/gateway/routes/{id}/rate-limit` | 🟡 | Set per-route rate limit |
| `DELETE` | `/gateway/routes/{id}/rate-limit` | 🟡 | Remove rate limit |
| `GET` | `/gateway/routes/{id}/cors` | 🟡 | Get CORS policy |
| `PUT` | `/gateway/routes/{id}/cors` | 🟡 | Set CORS policy |

---

## Schema / SDK / Spec

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/schema/graph` | ✅ | Schema dependency graph |
| `GET` | `/sdk/schema` | ✅ | Raw JSON schema for SDK generation |
| `GET` | `/sdk/typescript` | ✅ | Generated TypeScript types |
| `GET` | `/spec` | ✅ | Project spec (functions + schemas + agents) |

---

## Data Engine & Files (proxy)

These paths are forwarded to the Data Engine module in-process.

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `*` | `/db/{*path}` | ✅ | Proxy to Data Engine query handler |
| `*` | `/files/{*path}` | ✅ | Proxy to file storage handler |

---

## Records

Export, count, and manually prune execution records. The automated retention job (configured in `flux.toml` `[observability]`) deletes on a schedule; use `export` first if you need an archive before deletion.

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/records/export` | 🟡 | Stream records as JSONL/CSV; params: `before`, `after`, `function`, `errors_only`, `format` |
| `GET` | `/records/count` | 🟡 | Count matching records; same params as export |
| `DELETE` | `/records/prune` | 🟡 | Batch-delete matching records; param: `before` |

---

## API Keys

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/api-keys` | 🟡 | List API keys |
| `POST` | `/api-keys` | 🟡 | Create API key (returns key once) |
| `DELETE` | `/api-keys/{id}` | 🟡 | Revoke API key |
| `POST` | `/api-keys/{id}/rotate` | 🟡 | Rotate and return new key |

---

## Monitor

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/monitor/status` | 🟡 | Service health (all modules) |
| `GET` | `/monitor/metrics` | 🟡 | Request counts, error rate, p50/p95/p99 |
| `GET` | `/monitor/alerts` | 🟡 | List alert rules |
| `POST` | `/monitor/alerts` | 🟡 | Create alert rule |
| `DELETE` | `/monitor/alerts/{id}` | 🟡 | Delete alert rule |

---

## Events

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `POST` | `/events` | 🟡 | Publish an event |
| `GET` | `/events/subscriptions` | 🟡 | List subscriptions |
| `POST` | `/events/subscriptions` | 🟡 | Subscribe a function to an event type |
| `DELETE` | `/events/subscriptions/{id}` | 🟡 | Unsubscribe |

---

## Queue Management

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/queues` | 🟡 | List queues |
| `POST` | `/queues` | 🟡 | Create queue |
| `GET` | `/queues/{name}` | 🟡 | Get queue details |
| `DELETE` | `/queues/{name}` | 🟡 | Delete queue |
| `POST` | `/queues/{name}/messages` | 🟡 | Publish message |
| `GET` | `/queues/{name}/bindings` | 🟡 | List bindings |
| `POST` | `/queues/{name}/bindings` | 🟡 | Create binding |
| `POST` | `/queues/{name}/purge` | 🟡 | Purge all messages |
| `GET` | `/queues/{name}/dlq` | 🟡 | List dead-letter messages |
| `POST` | `/queues/{name}/dlq/replay` | 🟡 | Replay dead-letter messages |

---

## Schedules (Cron)

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/schedules` | 🟡 | List cron jobs |
| `POST` | `/schedules` | 🟡 | Create cron job |
| `DELETE` | `/schedules/{name}` | 🟡 | Delete cron job |
| `POST` | `/schedules/{name}/pause` | 🟡 | Pause without deleting |
| `POST` | `/schedules/{name}/resume` | 🟡 | Resume paused job |
| `POST` | `/schedules/{name}/run` | 🟡 | Trigger immediate run |
| `GET` | `/schedules/{name}/history` | 🟡 | Recent invocations |

---

## Agents

Agents are LLM-driven orchestrators that use functions as tools.
Each agent definition lives in `agents/` and references function names in its `tools` array.
Third-party integrations (Stripe, Slack, GitHub, etc.) are regular functions in `functions/` — there is no separate tool registry.

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/agents` | 🟡 | List agent definitions |
| `POST` | `/agents` | 🟡 | Register agent definition |
| `GET` | `/agents/{name}` | 🟡 | Get agent definition |
| `DELETE` | `/agents/{name}` | 🟡 | Delete agent |
| `POST` | `/agents/{name}/run` | 🟡 | Run agent with input; returns execution record |
| `POST` | `/agents/{name}/simulate` | 🟡 | Dry-run: show tool call plan without executing |

---

## Environments

| Method | Path | Status | Notes |
|--------|------|--------|-------|
| `GET` | `/environments` | 🟡 | List environments (production + development by default) |
| `POST` | `/environments` | 🟡 | Create environment |
| `DELETE` | `/environments/{name}` | 🟡 | Delete environment |
| `POST` | `/environments/clone` | 🟡 | Clone one environment into another |

---

## Execution-Plane Guard

These paths return `405 Method Not Allowed` on the API server — function execution only happens at the gateway root, never through the management API.

| Method | Path |
|--------|------|
| `*` | `/run`, `/run/{*path}` |
| `*` | `/invoke`, `/invoke/{*path}` |
| `*` | `/execute`, `/execute/{*path}` |
| `*` | `/functions/{name}/run` |
| `*` | `/functions/{name}/invoke` |

---

## Summary

| Status | Count | Section |
|--------|-------|---------|
| ✅ Implemented | 29 | Functions, Deployments, Secrets, Logs, Traces, Gateway CRUD, Schema/SDK, Data Engine proxy, Internal |
| 🟡 Stub | 43 | Gateway extras, API Keys, Monitor, Events, Queues, Schedules, Agents, Environments |
| 🔒 Internal-only | 7 | `/internal/*` — runtime and gateway use only |

**Primitives in Flux:** Function · Database · Queue · Agent  
**Storage:** Not a primitive — use an S3/GCS/R2 SDK inside a function. Flux records the call as an `ExternalCall` automatically.  
**Multiple databases:** Not supported — one Postgres instance per Flux server. Connect to a second DB manually inside a function; the query appears in the execution trace.  
**Removed primitives:** Workflow (use agent with ordered tool calls) · Tool (use a function that wraps the SDK)

