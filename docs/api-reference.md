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

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/health` | ✅ | Liveness check — `{"status":"ok"}` |
| `GET` | `/flux/api/version` | ✅ | Build info — service, commit SHA, build time |
| `GET` | `/flux/api/openapi.json` | ✅ | OpenAPI 3.0 spec JSON |
| `GET` | `/flux/api/openapi/ui` | ✅ | Swagger UI |
| `GET` | `/flux/api/tools/oauth/callback` | ✅ | OAuth redirect target after provider auth |

---

## Functions

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/functions` | ✅ | List all functions in the project |
| `POST` | `/flux/api/functions` | ✅ | Create a function record |
| `GET` | `/flux/api/functions/{id}` | ✅ | Get a single function |
| `DELETE` | `/flux/api/functions/{id}` | ✅ | Delete a function and all its deployments |
| `POST` | `/flux/api/functions/deploy` | ✅ | Deploy a function bundle (CLI upload, 10 MB limit) |
| `GET` | `/flux/api/spec` | ✅ | Full project spec — all functions + their types |

---

## Deployments

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `POST` | `/flux/api/deployments` | ✅ | Create a new deployment version |
| `GET` | `/flux/api/deployments/list/{function_id}` | ✅ | List all versions of a function |
| `POST` | `/flux/api/deployments/{id}/activate/{version}` | ✅ | Promote a version to active |

---

## Secrets

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/secrets` | ✅ | List all secret keys (values redacted) |
| `POST` | `/flux/api/secrets` | ✅ | Create a secret |
| `PUT` | `/flux/api/secrets/{key}` | ✅ | Update a secret value |
| `DELETE` | `/flux/api/secrets/{key}` | ✅ | Delete a secret |

---

## Logs & Traces

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/logs` | ✅ | List project logs (filter: `function`, `limit`, `since`) |
| `GET` | `/flux/api/traces` | ✅ | List recent traces (filter: `function`, `limit`) |
| `GET` | `/flux/api/traces/{request_id}` | ✅ | Full distributed trace for a single request |

---

## Gateway Routes

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/gateway/routes` | ✅ | List all gateway routes |
| `POST` | `/flux/api/gateway/routes` | ✅ | Create a route (`path`, `function`, `method`, `auth`) |
| `GET` | `/flux/api/gateway/routes/{id}` | 🟡 | Get a single route |
| `PATCH` | `/flux/api/gateway/routes/{id}` | ✅ | Update a route |
| `DELETE` | `/flux/api/gateway/routes/{id}` | ✅ | Delete a route |
| `PUT` | `/flux/api/gateway/routes/{id}/rate-limit` | 🟡 | Set per-route rate limit |
| `DELETE` | `/flux/api/gateway/routes/{id}/rate-limit` | 🟡 | Remove per-route rate limit |
| `GET` | `/flux/api/gateway/routes/{id}/cors` | 🟡 | Get CORS policy for a route |
| `PUT` | `/flux/api/gateway/routes/{id}/cors` | 🟡 | Set CORS policy for a route |
| `POST` | `/flux/api/gateway/middleware` | 🟡 | Attach middleware to a route |
| `DELETE` | `/flux/api/gateway/middleware/{route}/{type}` | 🟡 | Remove middleware from a route |

---

## API Keys

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/api-keys` | 🟡 | List API keys |
| `POST` | `/flux/api/api-keys` | 🟡 | Create an API key |
| `DELETE` | `/flux/api/api-keys/{id}` | 🟡 | Revoke an API key |
| `POST` | `/flux/api/api-keys/{id}/rotate` | 🟡 | Rotate an API key |

---

## Schema & SDK

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/schema/graph` | ✅ | Database schema as a relationship graph |
| `GET` | `/flux/api/sdk/schema` | ✅ | Raw SDK schema JSON |
| `GET` | `/flux/api/sdk/typescript` | ✅ | Generated TypeScript SDK source |

---

## Tools & Integrations

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/tools` | ✅ | List all available tools from catalog |
| `GET` | `/flux/api/tools/connected` | ✅ | List connected (authenticated) integrations |
| `GET` | `/flux/api/tools/{tool}` | 🟡 | Get details of one tool from catalog |
| `POST` | `/flux/api/tools/connect` | 🟡 | Connect an integration (app name in JSON body) |
| `POST` | `/flux/api/tools/connect/{provider}` | ✅ | Connect via provider slug (starts OAuth flow) |
| `DELETE` | `/flux/api/tools/disconnect/{provider}` | ✅ | Disconnect an integration |
| `POST` | `/flux/api/tools/run` | 🟡 | Execute a tool action via API |

---

## Storage

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/storage/provider` | ✅ | Get configured storage provider |
| `PUT` | `/flux/api/storage/provider` | ✅ | Set / update storage provider config |
| `DELETE` | `/flux/api/storage/provider` | ✅ | Remove storage provider config |
| `POST` | `/flux/api/storage/presign` | ✅ | Generate a presigned upload/download URL |

---

## Database (Data Engine Proxy)

All requests are proxied to the Data Engine service.

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `ANY` | `/flux/api/db/{*path}` | ✅ | Proxy to Data Engine — query, schema, migrations, etc. |
| `ANY` | `/flux/api/files/{*path}` | ✅ | Proxy to Data Engine file storage endpoints |

---

## Monitor

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/monitor/status` | 🟡 | Service health summary (all 5 subsystems) |
| `GET` | `/flux/api/monitor/metrics` | 🟡 | Aggregate metrics — request count, error rate, p50/p95/p99 |
| `GET` | `/flux/api/monitor/alerts` | 🟡 | List configured alerts |
| `POST` | `/flux/api/monitor/alerts` | 🟡 | Create an alert rule |
| `DELETE` | `/flux/api/monitor/alerts/{id}` | 🟡 | Delete an alert rule |

---

## Events

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `POST` | `/flux/api/events` | 🟡 | Publish a platform event |
| `GET` | `/flux/api/events/subscriptions` | 🟡 | List event subscriptions |
| `POST` | `/flux/api/events/subscriptions` | 🟡 | Create an event subscription |
| `DELETE` | `/flux/api/events/subscriptions/{id}` | 🟡 | Delete an event subscription |

---

## Queue Management

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/queues` | 🟡 | List all queues |
| `POST` | `/flux/api/queues` | 🟡 | Create a queue |
| `GET` | `/flux/api/queues/{name}` | 🟡 | Get queue details |
| `DELETE` | `/flux/api/queues/{name}` | 🟡 | Delete a queue |
| `POST` | `/flux/api/queues/{name}/messages` | 🟡 | Publish a message to a queue |
| `GET` | `/flux/api/queues/{name}/bindings` | 🟡 | List queue bindings |
| `POST` | `/flux/api/queues/{name}/bindings` | 🟡 | Create a queue binding |
| `POST` | `/flux/api/queues/{name}/purge` | 🟡 | Purge all messages from a queue |
| `GET` | `/flux/api/queues/{name}/dlq` | 🟡 | List dead-letter queue entries |
| `POST` | `/flux/api/queues/{name}/dlq/replay` | 🟡 | Replay dead-letter messages |

---

## Schedules

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/schedules` | 🟡 | List all scheduled jobs |
| `POST` | `/flux/api/schedules` | 🟡 | Create a scheduled job |
| `DELETE` | `/flux/api/schedules/{name}` | 🟡 | Delete a scheduled job |
| `POST` | `/flux/api/schedules/{name}/pause` | 🟡 | Pause a scheduled job |
| `POST` | `/flux/api/schedules/{name}/resume` | 🟡 | Resume a paused scheduled job |
| `POST` | `/flux/api/schedules/{name}/run` | 🟡 | Trigger a scheduled job immediately |
| `GET` | `/flux/api/schedules/{name}/history` | 🟡 | Execution history for a schedule |

---

## Agents

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/agents` | 🟡 | List all agents |
| `POST` | `/flux/api/agents` | 🟡 | Create an agent |
| `GET` | `/flux/api/agents/{name}` | 🟡 | Get agent details |
| `DELETE` | `/flux/api/agents/{name}` | 🟡 | Delete an agent |
| `POST` | `/flux/api/agents/{name}/run` | 🟡 | Run an agent |
| `POST` | `/flux/api/agents/{name}/simulate` | 🟡 | Simulate an agent run (dry-run) |

---

## Environments

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/environments` | 🟡 | List environments (returns `production`, `development` by default) |
| `POST` | `/flux/api/environments` | 🟡 | Create an environment |
| `DELETE` | `/flux/api/environments/{name}` | 🟡 | Delete an environment |
| `POST` | `/flux/api/environments/clone` | 🟡 | Clone one environment into another |

---

## Internal (service-token protected)

These routes are called by the runtime and gateway — never by the CLI or dashboard directly.  
Header required: `X-Service-Token: <INTERNAL_SERVICE_TOKEN>`

| Method | Path | Status | Description |
|--------|------|--------|-------------|
| `GET` | `/flux/api/internal/secrets` | 🔒✅ | Fetch runtime secrets for a project |
| `GET` | `/flux/api/internal/bundle` | 🔒✅ | Fetch compiled function bundle by deployment ID |
| `GET` | `/flux/api/internal/introspect` | 🔒✅ | Project introspection (schema, functions) for runtime |
| `GET` | `/flux/api/internal/logs` | 🔒✅ | Read logs (internal) |
| `POST` | `/flux/api/internal/logs` | 🔒✅ | Write execution logs from runtime |
| `GET` | `/flux/api/internal/functions/resolve` | 🔒✅ | Resolve a function name to its active deployment |
| `POST` | `/flux/api/internal/cache/invalidate` | 🔒✅ | Trigger gateway route cache invalidation |

---

## Gateway — Function Invocation

These live at the **root** (`http://localhost:4000`), not under `/flux/api`. They are served by the gateway module, not the API module.

| Method | Path | Description |
|--------|------|-------------|
| `ANY` | `/{function_name}` | Invoke a function by its gateway route |
| `ANY` | `/{*path}` | Wildcard — matched against configured gateway routes |

---

## Execution-plane Guard

These paths are blocked at the API module level with `405 Method Not Allowed` and a clear error message. Function invocation must go through the gateway.

| Path |
|------|
| `/run`, `/run/{*path}` |
| `/invoke`, `/invoke/{*path}` |
| `/execute`, `/execute/{*path}` |
| `/functions/{name}/run` |
| `/functions/{name}/invoke` |

---

## Summary counts

| Category | ✅ Implemented | 🟡 Stub | Total |
|----------|---------------|---------|-------|
| Utility | 5 | 0 | 5 |
| Functions | 6 | 0 | 6 |
| Deployments | 3 | 0 | 3 |
| Secrets | 4 | 0 | 4 |
| Logs & Traces | 3 | 0 | 3 |
| Gateway Routes | 5 | 6 | 11 |
| API Keys | 0 | 4 | 4 |
| Schema & SDK | 3 | 0 | 3 |
| Tools | 3 | 4 | 7 |
| Storage | 4 | 0 | 4 |
| Database Proxy | 2 | 0 | 2 |
| Monitor | 0 | 5 | 5 |
| Events | 0 | 4 | 4 |
| Queue Management | 0 | 10 | 10 |
| Schedules | 0 | 7 | 7 |
| Agents | 0 | 6 | 6 |
| Environments | 0 | 4 | 4 |
| Internal | 7 | 0 | 7 |
| **Total** | **45** | **50** | **95** |
