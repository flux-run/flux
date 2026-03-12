# Gateway

> **Internal architecture doc.** This describes the Gateway service implementation
> for contributors. For user-facing docs, see [framework.md](framework.md).

---

## Overview

| Property | Value |
|---|---|
| Service name | `flux-gateway` |
| Role | Edge routing, auth, rate limiting, trace root creation |
| Tech | Rust, Axum, reqwest |
| Default port | `:4000` |
| Exposed to internet | **Yes** — the only public-facing service |

The Gateway is the single entry point for all user traffic. It routes requests
to functions, enforces authentication and rate limits, and creates the root
span for every execution record.

```
Client (HTTPS via TLS terminator)
     │
     ▼
 Gateway :4000
     ├── Route resolution (in-memory RouteSnapshot)
     ├── Authentication (JWT / API key)
     ├── Rate limiting (per-tenant, per-route)
     ├── Trace root creation (x-request-id)
     │
     ▼
 Runtime :8083  (function execution)
```

---

## Trust boundary

Only the Gateway is exposed to the public internet. Runtime, Data Engine, API,
and Queue accept traffic only from the internal network, verified via
`X-Service-Token`.

---

## Local mode

In `flux dev`, the Gateway runs with `LOCAL_MODE=true`:
- Skips tenant resolution (no subdomain routing)
- Disables JWT auth
- Routes directly to localhost Runtime
- Same routing logic, just bypassed multi-tenant lookup

---

## Route snapshot

The Gateway maintains an in-memory hash map of routes, refreshed every 60
seconds from the API service:

```
RouteSnapshot {
  (tenant_id, function_name) → RouteRecord {
    function_id, runtime_url, auth_mode, rate_limit, middleware
  }
}
```

- O(1) lookup per request
- Updated via `GET /internal/routes` from API service
- If snapshot is empty at startup, Gateway returns 503 (not 404)
- `SKIP_SNAPSHOT_READY_CHECK` env var available for dev

---

## Request lifecycle

1. **Receive request** — validate `Content-Length` < `MAX_REQUEST_SIZE_BYTES` (default 10MB)
2. **Route resolution** — match `POST /<function_name>` against RouteSnapshot
3. **Authentication** — validate JWT or API key based on route auth mode
4. **Rate limit check** — in-memory per-tenant, per-route counters
5. **Create trace root** — generate `x-request-id`, write to `execution_records`
6. **Forward to Runtime** — `POST /execute` with request context + `x-request-id`
7. **Return response** — echo `x-request-id` in response headers

The trace root write is async (fire-and-forget) to avoid blocking the hot path.

---

## Authentication

| Mode | How it works |
|---|---|
| `none` | No auth (public endpoints) |
| `api_key` | Validate `Authorization: Bearer flux_*` against API service |
| `jwt` | Validate Firebase JWT, extract claims, set `ctx.user` |

JWKS is cached in memory (24h TTL, invalidated on 404 for key rotation).

---

## Rate limiting

- In-memory counters per (tenant, route)
- Configurable per route via API service
- Rejects with `429 Too Many Requests`
- Checked before execution — no wasted compute on rate-limited requests

---

## Caching

Read-only query responses cached at the gateway layer:

- **Single-flight concurrency** — identical queries coalesce into one backend call
- **Role-aware cache isolation** — cache key includes JWT role claim
- **Zero-copy sharing** — `Arc<Bytes>` + `Arc<HeaderMap>`
- Cache status exposed via `x-cache` header (`HIT` / `MISS` / `BYPASS`)
- Default TTL: 30 seconds, writes invalidate affected table cache immediately

---

## Health checks

| Endpoint | Response | Used by |
|---|---|---|
| `GET /health` | 200 if snapshot loaded, 503 if empty | Load balancer, Kubernetes |

Health checks are native gateway routes, not functions.

---

## Configuration

| Env var | Default | Description |
|---|---|---|
| `PORT` | `4000` | HTTP listen port |
| `RUNTIME_URL` | `http://localhost:8083` | Runtime service URL |
| `CONTROL_PLANE_URL` | `http://localhost:8080` | API service URL |
| `DATABASE_URL` | — | Postgres for trace root writes |
| `INTERNAL_SERVICE_TOKEN` | — | Shared service-to-service secret |
| `LOCAL_MODE` | `false` | Skip tenant resolution |
| `MAX_REQUEST_SIZE_BYTES` | `10485760` (10MB) | Request body limit |
| `SNAPSHOT_REFRESH_SECS` | `60` | Route snapshot refresh interval |
| `RUNTIME_TIMEOUT_SECS` | `30` | Timeout for runtime calls |

---

## TLS termination

The Gateway expects HTTP traffic. TLS must be terminated before it:

```
Client (HTTPS) → Cloud Run / ALB / Nginx (TLS) → Gateway (HTTP) → Runtime (HTTP)
```

---

## Production checklist

Before exposing to real traffic:

- [ ] TLS terminated at load balancer (not gateway)
- [ ] `INTERNAL_SERVICE_TOKEN` set and rotated
- [ ] `MAX_REQUEST_SIZE_BYTES` configured for your use case
- [ ] Rate limits configured per route
- [ ] Health check wired to load balancer
- [ ] Monitoring: `p99 < 500ms`, `5xx rate < 1%`
- [ ] Snapshot readiness verified (503 during cold start, not 404)

---

*Source: `gateway/src/`. For the full architecture, see
[framework.md §4](framework.md#4-architecture).*
