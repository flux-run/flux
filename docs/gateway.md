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
| Default port | `:8081` |
| Exposed to internet | **Yes** — the only public-facing service |

The Gateway is the single entry point for all user traffic. It accepts any
domain and routes purely by HTTP method + path. No tenant resolution, no
subdomain routing. Routes are registered in the database and kept in memory
via Postgres LISTEN/NOTIFY.

```
Client (HTTPS via TLS terminator)
     │
     ▼
 Gateway :8081
     ├── Route resolution  — (METHOD, /path) → RouteRecord
     ├── Authentication    — none | api_key | jwt
     ├── Rate limiting     — per-route × per-client-IP token bucket
     ├── JSON Schema validation (optional, per-route)
     ├── Trace root        — x-request-id + fire-and-forget DB write
     │
     ▼
 Runtime :8083  (POST /execute)
```

---

## Trust boundary

Only the Gateway is exposed to the public internet. Runtime, API, and Queue
accept traffic only from the internal network, verified via `X-Service-Token`.

---

## Local mode

In `flux dev`, the Gateway runs with `LOCAL_MODE=true`:
- Auth is skipped — `AuthContext::Dev` injected for all requests
- Rate limiting still applies
- Same routing logic, same trace writes
- Set via `LOCAL_MODE=true` or `FLUX_LOCAL=true`

---

## Route snapshot

The Gateway holds all registered routes in a `HashMap<(METHOD, /path), RouteRecord>`
loaded from Postgres.

```
SnapshotData {
  routes: HashMap<(String, String), RouteRecord>
  // key: ("POST", "/create_user")
}
```

**How it stays current:**
1. **Startup** — full load from DB before accepting traffic
2. **LISTEN/NOTIFY** — Postgres trigger fires `NOTIFY route_changes` on every
   INSERT, UPDATE, DELETE in `routes`. The gateway listener refreshes immediately.
3. **Reconnect** — if the NOTIFY connection drops, it reconnects with exponential
   back-off (1s → 2s → ... → 30s) and refreshes on reconnect to catch missed changes.

No polling. No background timer. Zero steady-state DB load.

If the snapshot is empty at startup, `/readiness` returns 503. Set
`SKIP_SNAPSHOT_READY_CHECK=1` to bypass in dev.

---

## Request lifecycle

1. **Content-Length guard** — reject > `MAX_REQUEST_SIZE_BYTES` early (no body read)
2. **Route resolution** — `(METHOD, /path)` lookup in snapshot → 404 if missing
3. **CORS preflight** — OPTIONS fast path if `cors_enabled` on route
4. **Authentication** — `none` / `api_key` / `jwt` based on route `auth_type`
5. **Rate limit** — token bucket per `(route_id, client_ip)` → 429 if exceeded
6. **Read body** — collect bytes → 413 if over limit
7. **JSON Schema validation** — optional per-route → 400 if invalid
8. **Trace root** — resolve `x-request-id`, fire-and-forget write to `trace_requests`
9. **Forward to Runtime** — `POST {RUNTIME_URL}/execute` with context headers
10. **Return response** — echo `x-request-id` in response headers

Steps 1–3 never read the body. Auth failure short-circuits before rate limiting.

---

## Authentication

| `auth_type` | How it works |
|---|---|
| `none` | Public endpoint, no credentials required |
| `api_key` | `Authorization: Bearer <key>` or `X-API-Key` — SHA-256 hash checked against `api_keys` table |
| `jwt` | `Authorization: Bearer <token>` — verified against per-route JWKS URL with in-process key cache |

JWKS keys are cached in memory per URL. On `kid` miss the cache is invalidated
and re-fetched once (handles key rotation).

Auth context is forwarded to the Runtime as headers:
- `X-User-Id` — JWT `user_id` or `sub` claim
- `X-JWT-Claims` — full claim payload (JSON)

---

## Rate limiting

- In-memory token bucket per `(route_id, client_ip)`
- Per-route override via `rate_limit` column (falls back to `RATE_LIMIT_PER_SEC`)
- Rejects with `429 Too Many Requests` before reading the body
- State is process-local — each Gateway pod has independent counters
  (acceptable: rare edge case, no coordination overhead)

---

## Health checks

| Endpoint | When | Used by |
|---|---|---|
| `GET /health` | Always 200 | Load balancer liveness probe |
| `GET /readiness` | 200 once snapshot loaded, 503 until then | Kubernetes readiness probe |

Do **not** wire the load balancer to `/readiness` — that would take the
gateway out of rotation on every NOTIFY reconnect. Use `/health` for the LB
and `/readiness` for Kubernetes only.

---

## Configuration

| Env var | Default | Required | Description |
|---|---|---|---|
| `DATABASE_URL` | — | ✅ | Postgres — route snapshot + trace writes |
| `INTERNAL_SERVICE_TOKEN` | — | ✅ | Service-to-service shared secret |
| `PORT` | `8081` | | HTTP listen port |
| `RUNTIME_URL` | `http://localhost:8083` | | Runtime execution service |
| `CONTROL_PLANE_URL` | `http://localhost:8080` | | API service (future admin ops) |
| `MAX_REQUEST_SIZE_BYTES` | `10485760` (10 MB) | | Request body limit |
| `RUNTIME_TIMEOUT_SECS` | `30` | | HTTP timeout for runtime calls |
| `RATE_LIMIT_PER_SEC` | `50` | | Default per-route per-IP rate limit |
| `LOCAL_MODE` / `FLUX_LOCAL` | `false` | | Skip auth (dev) |

Copy `gateway/.env.example` → `gateway/.env` for local development.

---

## TLS termination

The Gateway expects plain HTTP. TLS must be terminated upstream:

```
Client (HTTPS) → Cloud Run / ALB / Nginx (TLS) → Gateway :8081 (HTTP) → Runtime :8083 (HTTP)
```

---

## Source layout

```
gateway/src/
  config.rs          — env var loading
  state.rs           — shared GatewayState
  main.rs            — startup only
  router.rs          — 3 routes: /health, /readiness, /{*path}
  snapshot/
    types.rs         — RouteRecord, SnapshotData
    store.rs         — GatewaySnapshot: startup load + NOTIFY listener
  auth/
    mod.rs           — AuthContext + check()
    api_key.rs       — DB key validation
    jwt.rs           — JwksCache + verify()
  rate_limit/mod.rs  — token bucket
  trace/mod.rs       — request-ID + fire-and-forget DB write
  forward/mod.rs     — POST /execute to runtime
  handlers/
    health.rs        — GET /health
    readiness.rs     — GET /readiness
    dispatch.rs      — pipeline orchestrator (steps 1–10 above)
```

---

## Production checklist

- [ ] TLS terminated at load balancer
- [ ] `INTERNAL_SERVICE_TOKEN` set and rotated regularly
- [ ] `MAX_REQUEST_SIZE_BYTES` tuned for your use case
- [ ] `RATE_LIMIT_PER_SEC` set per route in DB
- [ ] Load balancer health check on `GET /health`
- [ ] Kubernetes readiness probe on `GET /readiness`
- [ ] DB trigger `route_change_notify` applied (migration `20260312000029`)
- [ ] `DATABASE_URL` Postgres user has `LISTEN` privilege

---

*Source: `gateway/src/`. For the full architecture, see
[framework.md §4](framework.md#4-architecture).*


