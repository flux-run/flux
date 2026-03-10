# Fluxbase Gateway Module

## Overview

The **Gateway** is the **edge runtime orchestrator** and primary control plane entry point for Fluxbase. It is far more than a reverse proxy — it is the system responsible for:

- **Trace Root Creation** — Every incoming request creates an observable trace tree
- **Multi-Tenant Routing** — Resolves client requests to the correct tenant + function based on subdomain
- **Authentication & Authorization** — Enforces JWT, API key, and rate-limit policies before execution
- **Intelligent Caching** — Edge-cached query responses with single-flight deduplication to prevent thundering herd
- **Real-time Event Streaming** — Proxies Server-Sent Events for live subscriptions
- **Request Coordination** — Forwards authenticated, traced requests to Runtime for execution

**Core Design Philosophy**: Observable by construction. Every request generates a trace root that automatically propagates through Runtime → Data-Engine → Storage/Tools, capturing the complete execution graph.

The gateway achieves low-latency, high-reliability request handling through:
- **In-memory routing snapshot** — All routes cache in memory, refreshed every 60 seconds (O(1) lookup)
- **Single-flight concurrency** — Multiple identical queries coalesce into one backend call (prevents cache stampede)
- **Zero-copy response caching** — Responses stored as `Arc<Bytes>` + `Arc<HeaderMap>` for instant clones (nanosecond sharing)
- **Role-aware cache isolation** — Cache key includes JWT `role` claim (prevents RLS/CLS leaks)
- **Efficient header filtering** — Sensitive headers stripped before caching to prevent cross-request leaks

---

## Why the Gateway Exists

Fluxbase's core promise is **observability by construction**. The gateway is the architectural component that makes this possible. Here's why it exists:

### 1. **Trace Root Authority**
The gateway is the **only component that sees the original client request**. It has the authority to:
- Generate a unique `request_id` (correlation ID)
- Create the root span in the distributed trace
- Inject the trace context into all downstream services
- Associate the trace with the correct tenant and project before any work happens

Without this, traces would be fragmented across services with no unified view.

### 2. **Centralized Routing & Identity**
Instead of embedding routing logic in every function runtime, the gateway owns it:
- One source of truth for which request goes to which function
- Enables instant route changes (60s snapshot refresh) without redeploying code
- Allows traffic splitting & canary rollouts at the gateway layer
- Isolates tenant identity resolution (no tenant confusion in tenant boundaries)

### 3. **Authentication Before Execution**
The gateway validates credentials **before invoking runtime**, preventing:
- Wasted execution cycles on unauthorized requests
- Accidental exposure of function secrets to unsauthenticated clients
- Auth failures creating partial state in databases

### 4. **Rate Limiting at the Edge**
Rate limits are enforced at the gateway, not after runtime invocation:
- Burst traffic is rejected instantly (no queuing overhead)
- Prevents DDoS from reaching execution engines
- Enables per-tenant, per-route rate limits with centralized visibility

### 5. **Query Cache as Shared Infrastructure**
Read-only data queries are cached at the gateway (not in function code):
- Single query result shared across all concurrent requests (single-flight)
- No duplicate backend calls on cache hits
- Invalidation centralized (no cache invalidation scattered across functions)
- Role-aware to prevent permission leaks

In summary: **The gateway is the control plane**. It provides three core guarantees:

1. ✅ Every request is traced automatically (observable roots)
2. ✅ Routing decisions are centralized and consistent (no distributed routing logic)
3. ✅ Authentication, rate limiting, and caching occur before runtime execution (fail fast, share infrastructure)

---

## TLS Termination Policy

**All external traffic MUST terminate TLS before reaching the gateway.**

The Gateway is an internal component that expects HTTP traffic. TLS termination should occur at a higher layer:

```
Client (HTTPS)
    ↓
Cloud Run / ALB / Nginx (TLS termination)
    ↓
Gateway (HTTP internal)
    ↓
Runtime, Data-Engine, Queue (HTTP internal)
```

**Expected Headers from TLS Terminator**:

The gateway validates these headers to reconstruct the original client request:

| Header | Purpose | Example |
|--------|---------|----------|
| `X-Forwarded-Proto` | Original protocol (must be "https") | `https` |
| `X-Forwarded-For` | Client IP address (for logging/rate limiting) | `192.0.2.1` |
| `X-Forwarded-Host` | Original host from client request | `acme-org.fluxbase.dev` |

**Validation**:

```rust
// Gateway validation
if let Some(proto) = req.headers().get("x-forwarded-proto") {
    if proto != "https" {
        return Err(400 Bad Request);  // Reject non-HTTPS clients
    }
}
```

**Why This Matters**:

Without TLS termination validation, developers may accidentally:
- Deploy the gateway directly on the internet (security risk)
- Send plaintext authentication tokens over HTTP
- Expose sensitive request data in logs

---

## Architecture

### High-Level Components

```
┌────────────────────────────────────────────────────────────────────┐
│                         Gateway Service (8081)                      │
├────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌──────────────────────┐         ┌──────────────────────┐         │
│  │  Router Layer        │         │  Request Pipeline    │         │
│  │  • /health           │         │  • Identity Resolver │         │
│  │  • /version          │         │  • Auth Middleware   │         │
│  │  • CORS Config       │         │  • Rate Limiting     │         │
│  └──────────────────────┘         │  • Analytics Logging │         │
│                                   └──────────────────────┘         │
│                                                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │                      Route Handlers                         │   │
│  ├─────────────────────────────────────────────────────────────┤   │
│  │                                                              │   │
│  │  1. Proxy Handler          ─────────► RuntimeURL            │   │
│  │     (Serverless Functions)            Async Job Queueing   │   │
│  │                                                              │   │
│  │  2. Data-Engine Proxy      ─────────► DataEngineURL        │   │
│  │     (DB Query / File Ops)             Edge Cache (QueryCache) │   │
│  │     • Single-flight cache             • Role-aware sharing  │   │
│  │     • Expires after 30s               • Table invalidation  │   │
│  │                                                              │   │
│  │  3. Events Handler         ─────────► API URL               │   │
│  │     (SSE Subscription)                Transparent proxy    │   │
│  │                                                              │   │
│  │  4. Cache Control          ─────────► POST /internal/*     │   │
│  │     (Invalidate / Stats)               Service-token auth  │   │
│  │                                                              │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                      │
│  ┌──────────────────────┐  ┌──────────────────────┐                │
│  │  Caching Layer       │  │  Identity Resolution │                │
│  │  ┌────────────────┐  │  │  ┌────────────────┐   │                │
│  │  │ Snapshot Cache │  │  │  │ Tenant Lookup  │   │                │
│  │  │ (Routes)       │  │  │  │ by Subdomain   │   │                │
│  │  │ 60s TTL        │  │  │  │ (Reserved list)│   │                │
│  │  └────────────────┘  │  │  └────────────────┘   │                │
│  │  ┌────────────────┐  │  │  ┌────────────────┐   │                │
│  │  │ Query Cache    │  │  │  │ JWT Validation │   │                │
│  │  │ (Read-only)    │  │  │  │ + JWKS Caching │   │                │
│  │  │ 30s TTL        │  │  │  │ (OIDC support)│   │                │
│  │  └────────────────┘  │  │  └────────────────┘   │                │
│  │  ┌────────────────┐  │  │  ┌────────────────┐   │                │
│  │  │ JWKS Cache     │  │  │  │ API Key Auth   │   │                │
│  │  │ (Public Keys)  │  │  │  │ Validation     │   │                │
│  │  │ (Persistent)   │  │  │  │                │   │                │
│  │  └────────────────┘  │  │  └────────────────┘   │                │
│  └──────────────────────┘  └──────────────────────┘                │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  Database & External Services                                │   │
│  │  • PostgreSQL (Neon)  — routing config, audit logs           │   │
│  │  • Runtime Service    — serverless execution                │   │
│  │  • Data-Engine        — structured query + file ops         │   │
│  │  • API Service        — SSE events, management calls        │   │
│  │  • Queue Service      — job contract for async execution    │   │
│  └──────────────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────────────┘
```

### Core Modules

| Module | Purpose | Key Types |
|--------|---------|-----------|
| **config.rs** | Environment & configuration loading | `Config` — database, service URLs, ports, secrets |
| **state.rs** | Shared application state (DI context) | `GatewayState`, `SharedState` — HTTP client, pools, caches |
| **router.rs** | HTTP endpoint registration | Axum `Router` — request to handler mapping |
| **routes/** | HTTP handlers | `proxy_handler`, `data_engine::proxy_handler`, `events::stream` |
| **middleware/** | Request processing pipeline | Identity resolver, auth, rate-limiting, analytics |
| **cache/** | Multi-tier caching | Snapshot (routes), QueryCache (data), JWKS (public keys) |
| **services/** | Business logic | `RouteRecord` — route metadata + constraints |

---

## Request Lifecycle

Understanding the complete lifecycle of a single HTTP request is key to understanding Fluxbase:

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        CLIENT REQUEST FLOW                                   │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                              │
│   1. CLIENT SENDS REQUEST                                                  │
│      ─────────────────────                                                  │
│      POST /api/users/create                                                │
│      Host: acme-org.fluxbase.dev                                          │
│      Authorization: Bearer <jwt>                                           │
│      Content-Type: application/json                                        │
│      { "name": "Alice", ... }                                             │
│                                                                              │
│          │                                                                  │
│          ├──────────────────► GATEWAY (port 8081)                         │
│          │                                                                  │
│   2. GATEWAY.RECEIVE                                                       │
│      ────────────────                                                      │
│      ├─ Parse HTTP headers                                                │
│      ├─ Generate x-request-id or use existing                            │
│      └─ Create root trace:                                               │
│          INSERT INTO platform_logs (..., span_type='start')              │
│                                                                              │
│          ↓ (x-request-id = "550e8400-e29b-41d4-a716-446655440000")       │
│                                                                              │
│   3. IDENTITY_RESOLVER                                                     │
│      ──────────────────                                                    │
│      ├─ Extract subdomain from Host: "acme-org"                          │
│      ├─ Normalize: lowercase, strip non-[a-z0-9-], collapse dashes       │
│      ├─ Check reserved words (prevents platform domain hijacking)        │
│      ├─ Lookup tenant_id from memory snapshot                            │
│      └─ Attach ResolvedIdentity to request context                       │
│                                                                              │
│          ↓ (tenant_id resolved)                                            │
│                                                                              │
│   4. ROUTE_LOOKUP                                                          │
│      ────────────────                                                      │
│      ├─ Query memory snapshot: (tenant_id, "POST", "/api/users/create")  │
│      └─ Load RouteRecord with function metadata:                         │
│          { function_id, is_async, auth_type="jwt", cors_enabled,        │
│            rate_limit=100, jwks_url=..., ... }                          │
│                                                                              │
│          ↓ (route found)                                                   │
│                                                                              │
│   5. AUTHENTICATION                                                        │
│      ──────────────────                                                    │
│      ├─ Extract "Bearer <jwt>" from Authorization header                │
│      ├─ Validate signature using cached JWKS                            │
│      ├─ Check audience, issuer                                           │
│      ├─ Extract "role" claim for cache isolation                        │
│      └─ If invalid: 401 Unauthorized (fail fast)                        │
│                                                                              │
│          ↓ (JWT verified, role="admin")                                 │
│                                                                              │
│   6. RATE_LIMITING                                                         │
│      ──────────────                                                        │
│      ├─ Check token bucket for (tenant_id, route_id)                    │
│      ├─ If over limit: 429 Too Many Requests (fail fast)               │
│      └─ Otherwise: consume 1 token, continue                            │
│                                                                              │
│          ↓ (tokens available)                                              │
│                                                                              │
│   7. TRACE_SPAN_START (async, non-blocking)                              │
│      ──────────────────────                                               │
│      │ (Fire-and-forget task)                                            │
│      └─ INSERT INTO platform_logs                                        │
│         ( source='gateway',                                              │
│           span_type='route_matched',                                     │
│           resource_id=<function_id>,                                     │
│           message='POST /api/users/create',                             │
│           request_id=<x-request-id> )                                    │
│                                                                              │
│   8. REQUEST_FORWARD_TO_RUNTIME                                           │
│      ──────────────────────────                                           │
│      ├─ Forward headers: auth, x-request-id, x-tenant-id, x-project-id │
│      ├─ Forward body: { "name": "Alice", ... }                         │
│      ├─ Include x-service-token for service-to-service auth            │
│      └─ POST http://runtime:3000/api/users/create                      │
│                                                                              │
│          │                                                                  │
│          └──────────────────► RUNTIME SERVICE (port 3000)               │
│                         │                                                  │
│                         │ (Inside Runtime: function execution)            │
│                         ├─ Execute user function code                    │
│                         ├─ Call ctx.db.insert("users", ...)            │
│                         |    └─ Proxies to Data-Engine + logs query     │
│                         ├─ Call ctx.tool.gmail.send_email(...)         │
│                         |    └─ Calls Composio integration              │
│                         ├─ Call ctx.workflow.run(...)                    │
│                         │    └─ Chains other functions                  │
│                         └─ Return response { status: 201, body: {...} }│
│                                                                              │
│          │                                                                  │
│          └──────────────────► GATEWAY (receives response)               │
│                                                                              │
│   9. GATEWAY.RESPONSE (same x-request-id throughout)                     │
│      ──────────────────                                                    │
│      ├─ Status: 201 Created                                              │
│      ├─ Headers: Content-Type, X-Request-ID, <others>                  │
│      └─ Body: { "id": "user-123", "name": "Alice", ... }              │
│                                                                              │
│   10. TRACE_SPAN_END (async, non-blocking)                               │
│       ────────────────                                                     │
│       │ (Fire-and-forget task)                                           │
│       └─ INSERT INTO platform_logs                                       │
│          ( source='gateway',                                             │
│            span_type='complete',                                         │
│            request_id=<same x-request-id>,                              │
│            status=201,                                                   │
│            duration_ms=145 )                                             │
│                                                                              │
│   11. CLIENT RECEIVES RESPONSE                                            │
│       ───────────────────────                                             │
│       ├─ HTTP 201                                                        │
│       ├─ X-Request-ID: "550e8400-e29b-41d4-a716-446655440000"         │
│       └─ { "id": "user-123", "name": "Alice", ... }                  │
│                                                                              │
│───────────────────────────────────────────────────────────────────────────│
│                                                                              │
│  TRACE TREE IN PLATFORM_LOGS:                                             │
│  ═════════════════════════════════════════════════════════════════════════ │
│                                                                              │
│  request_id=550e8400-e29b-41d4-a716-446655440000                         │
│  │                                                                          │
│  ├─ gateway::receive                        [0ms]                         │
│  │  └─ tenant_id=5b5f77d1... (acme-org)                                 │
│  │                                                                          │
│  ├─ gateway::route_matched                  [1ms]                         │
│  │  └─ function_id=a7e3d... (/api/users/create)                        │
│  │                                                                          │
│  ├─ gateway::auth_passed                    [6ms]                         │
│  │  └─ role=admin (from JWT)                                            │
│  │                                                                          │
│  ├─ gateway::rate_limit_passed              [7ms]                         │
│  │                                                                          │
│  └─ runtime::execute_function               [7-145ms]                     │
│     │                                                                       │
│     ├─ function::create_user::start                                       │
│     │  └─ input={ "name": "Alice" }                                     │
│     │                                                                       │
│     ├─ data_engine::db.insert               [25-75ms]                     │
│     │  ├─ table=users                                                    │
│     │  ├─ query_hash=0x3a4d...                                          │
│     │  └─ duration_ms=50                                                │
│     │                                                                       │
│     ├─ composio::gmail.send_email           [95-135ms]                   │
│     │  ├─ to=alice@example.com                                          │
│     │  └─ duration_ms=40                                                │
│     │                                                                       │
│     └─ function::create_user::complete      [145ms]                      │
│        ├─ status=ok                                                       │
│        └─ output={ "id": "user-123" }                                   │
│
└─────────────────────────────────────────────────────────────────────────────┘
```

**Key Insight**: The `x-request-id` (request_id) is the golden thread that weaves all services together. Every log entry, every database query, every tool call is tagged with this ID, creating a unified trace tree.

---

## Trace Root Architecture

Fluxbase's observability is **built on trace roots, not log aggregation**. The gateway creates and manages these roots.

### Root Span Creation

When a request arrives at the gateway:

1. **Generate or Extract Request ID**
   ```
   x-request-id: <uuid>  (from client header or generated)
   ```

2. **Create Root Span**
   ```sql
   INSERT INTO platform_logs
   (id, parent_span_id, request_id, tenant_id, project_id, source, span_type, level, message)
   VALUES
   (<uuid>, NULL, <request_id::uuid>, <tenant_id>, <project_id>, 'gateway', 'start', 'info', 'gateway.receive')
   ```
   
   **Note**: `parent_span_id` is NULL for the root. Child spans set `parent_span_id = <gateway_root_span_id>` to create hierarchy.

3. **Propagate Request ID**
   - Forward `x-request-id` to every downstream service (Runtime, Data-Engine, API)
   - Every service appends spans with the same `request_id`
   - Result: unified trace tree

### Trace Propagation Chain

```
Client Request
    ↓
Gateway (creates root, generates x-request-id)
    ├─ Span: gateway.receive
    ├─ Span: gateway.identity_resolved
    ├─ Span: gateway.route_matched
    ├─ Span: gateway.auth_passed
    ├─ Span: gateway.rate_limit_passed
    └─ Header: x-request-id=<uuid>
    ↓
Runtime (receives same x-request-id)
    ├─ Span: runtime.execute_function
    ├─ Call: ctx.db.insert(...)
    │  └─ Forwarded to Data-Engine with x-request-id
    │     ├─ Span: data_engine.insert
    │     ├─ Span: data_engine.write_to_db
    │     └─ Response includes duration
    ├─ Call: ctx.tool.gmail.send(...)
    │  └─ Forwarded to tool provider with x-request-id
    │     ├─ Span: composio.send_email
    │     └─ Response
    ├─ Call: ctx.workflow.run(...)
    │  └─ Recursive invocation of another function
    │     └─ Span tree for that function
    └─ Span: runtime.execute_function.complete
         └─ Duration: 145ms
```

### Why This Model Matters

**Without trace roots:**
- Logs scattered across services with no connection
- "Did my function fail?" requires correlating 5+ independent log sources
- Debugging multi-step workflows becomes detective work

**With trace roots (Fluxbase approach):**
- Single request_id stitches all logs together
- Dashboard shows complete execution tree with timing
- Bottlenecks visible instantly (which step took 100ms?)
- Cost becomes visible (which tool call was expensive?)

### Span Types

Gateway logs five primary span types:

| Span Type | When | Purpose |
|-----------|------|----------|
| `start` | Request received | Root of trace tree |
| `route_matched` | Route resolution complete | Logs which function was invoked |
| `auth_passed` | JWT/API key validated | Logs authenticated role/principal |
| `rate_limit_passed` | Rate limit check passed | Log remaining tokens |
| `complete` | Response ready | Root completion with status + duration |

**Note**: `span_type` is a semantic category. Additional spans (e.g., `data_engine.query`, `workflow.invoke`, `tool.execute`) are generated by Runtime, Data-Engine, and other downstream services using the same `request_id`.

---

### Request ID Policy & Governance

Request IDs are the golden thread of observability. The gateway enforces a strict policy to prevent spoofing:

**Incoming Request Processing**:

1. **If `x-request-id` header is present**:
   - Validate it is a properly formatted UUID (uuid::Error parsing)
   - Reject malformed values with 400 Bad Request
   - Parse as `UUID` type (not String)
   - Use as the authoritative request_id
   - Forward unchanged to all downstream services

2. **If `x-request-id` header is absent**:
   - Generate a new UUID v7 (use `uuid::Uuid::now_v7()`)
   - UUID v7 provides **sortable timestamps** for trace ordering (compare UUIDs lexicographically = ordering by time)
   - Convert to String for HTTP transmission
   - Parse back to `UUID` for database operations
   - Inject into all downstream context as header

**Schema Enforcement**: `request_id UUID NOT NULL` in platform_logs **prevents invalid IDs at the database level**. Text-based request IDs create:
- Slower index lookups (string comparisons vs UUID binary comparison)
- Risk of storage of invalid values like "hello" or partial UUIDs
- Larger index footprint (UUID = 16 bytes, random strings = variable)

**Security Guarantee**: The gateway-provided `x-request-id` is **authoritative**. Downstream services must never replace it with a value from a nested call. This prevents:
- Malicious clients spoofing request IDs to access unrelated traces
- Accidental loss of trace continuity across service boundaries

#### Trace Context Compatibility (W3C Standard)

Fluxbase currently propagates tracing using internal headers (`x-request-id`, `x-parent-span-id`). To ensure compatibility with industry-standard observability tools, the gateway should also support the **W3C Trace Context** specification.

**Standard W3C Headers**:
```
traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01
tracestate: vendor=value
```

**Format**: `traceparent: version-trace-id-parent-id-flags`

**Mapping Strategy**:

| W3C Field | Fluxbase Field | Purpose |
|-----------|----------------|---------|
| trace-id (128-bit) | request_id (UUID) | Root trace identifier |
| parent-id (64-bit) | parent_span_id (UUID) | Parent span identifier |

**Gateway Behavior**:

1. **If `traceparent` header exists** → Extract trace-id and parent-id
2. Map trace-id → `request_id` (validate UUID format)
3. Map parent-id → `parent_span_id` for span relationships
4. Generate a **new span-id** for the gateway's own span
5. Forward updated `traceparent` downstream with gateway span-id

**Example Flow**:

```
Client sends:
  traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01

Gateway:
  ├─ Extract: trace-id=4bf92f3577b34da6a3ce929d0e0e4736
  ├─ Extract: parent-id=00f067aa0ba902b7
  ├─ Create new gateway span-id=a1b2c3d4e5f6g7h8
  └─ Forward downstream:
     traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-a1b2c3d4e5f6g7h8-01

Runtime receives same trace-id, can create its own child spans.
```

**External Tool Integration**:

This enables Fluxbase to integrate with industry-standard observability systems:
- **OpenTelemetry** — Parse W3C traceparent natively
- **Jaeger** — Ingest traces with standard context propagation
- **Datadog APM** — Inject Datadog trace context alongside W3C
- **Honeycomb** — Use W3C headers for cross-service tracing

**Implementation Priority**: Medium (enables future integrations without breaking internal architecture)

### Trace Reconstruction & Query

Fluxbase traces are not streamed—they are reconstructed from storage on-demand.

**Storage Model**: All spans (gateway, runtime, data-engine, tools) are persisted to a single `platform_logs` table:

```sql
SELECT * FROM platform_logs
WHERE request_id = '550e8400-e29b-41d4-a716-446655440000'
ORDER BY created_at ASC
```

**Reconstruction**: The **Fluxbase API** service rebuilds trace trees by:
1. Querying all spans for a request_id
2. Finding the root span (where `parent_span_id IS NULL`)
3. Building a tree by following `parent_span_id` pointers (child spans reference parent via this ID)
4. Computing cumulative latencies and critical path analysis per branch
5. Computing resource attribution per service and tool call

**Example**: For request_id=550e8400-..., the tree reconstruction uses:

```
gateway.receive (id=span-1, parent_span_id=NULL) [ROOT]
  ├─ gateway.auth_passed (id=span-2, parent_span_id=span-1)
  ├─ runtime.execute_function (id=span-3, parent_span_id=span-1)
     ├─ data_engine.db.insert (id=span-4, parent_span_id=span-3)
     │   └─ data_engine.write_to_postgres (id=span-5, parent_span_id=span-4)
     └─ composio.gmail.send (id=span-6, parent_span_id=span-3)
```

Without `parent_span_id`, you cannot construct this hierarchy — only a flat list ordered by timestamp.

**Important**: Runtime and Data-Engine services **must** capture the parent span ID from the incoming `x-parent-span-id` header and pass it when creating child spans.

**CLI Usage Example**:
```bash
$ flux trace 550e8400-e29b-41d4-a716-446655440000

gateway.receive                          0ms
├─ gateway.route_matched                 1ms      function_id=a7e3d
├─ gateway.auth_passed                   6ms      role=admin
├─ gateway.rate_limit_passed             7ms
└─ runtime.execute_function           7-145ms
   ├─ data_engine.db.insert          25-75ms      table=users
   │  └─ insert_rows=1
   ├─ composio.gmail.send_email      95-135ms     to=alice@example.com
   │  └─ status=sent
   └─ runtime.execute_function.complete  145ms    status=ok
```

This unified trace surfaces:
- Total latency (145ms)
- Bottleneck (Data-Engine query: 50ms)
- Tool cost (Gmail: 40ms)
- Function-specific work (10ms)

### Snapshot Cache Consistency & Freshness

The gateway maintains a memory snapshot of all routes. The consistency model balances freshness and performance:

**Refresh Mechanism**:

1. **Periodic Refresh** (every 60 seconds):
   - Background task queries `routes` table from database
   - Atomic swap: old snapshot → new snapshot
   - In-flight requests continue using old snapshot
   - New requests use new snapshot immediately

2. **Future: Event-Driven Refresh**:
   - Listen to `platform_logs` for route mutations
   - Trigger immediate refresh when route is created/updated
   - Reduces perceived latency from 60s to <1s

**Worst-Case Behavior**:
- Route is created at T=0
- Snapshot naturally refreshes at T=60 seconds
- Client requests between T=0 and T=60 receive 404 Not Found
- This is acceptable (manual refresh can reduce to <1s if needed)

**Important**: Routes are never hard-deleted (soft-deleted with `is_active=false`), so there are no silent failures—only explicit 404s during the grace period.

### Async Job Queue Path

For routes marked `is_async=true`, the gateway does not invoke the Runtime directly. Instead:

```
Client Request
  ↓
[Gateway]
  ├─ Identity, Auth, Rate Limit (same as sync)
  └─ is_async=true detected
       ↓
[Queue Service]
  ├─ Enqueue job with request payload
  ├─ Return 202 Accepted immediately
  └─ Job executed asynchronously by Runtime
```

**Benefits**:
- Client receives response in <100ms (job enqueue only)
- Function execution happens in background
- Request can continue even if function is slow/fails
- Better for webhooks, notifications, batch processing

**Reliability Guarantee** (At-Least-Once):
- Queue Service guarantees **at-least-once delivery**
- Functions may be executed multiple times on failure/retry
- **Functions MUST be idempotent**:
  ```javascript
  // Bad (not idempotent):
  ctx.db.insert("users", { id, name })  // Fails on second retry with duplicate key error
  
  // Good (idempotent):
  ctx.db.insertIgnoreDuplicate("users", { id, name })  // Or use upsert
  ```
- If Queue crashes, jobs are replayed until success
- If Runtime crashes mid-execution, job restarred
- Client never knows about retries (202 response is final)

---



## Key Features

### 1. Multi-Tenant Routing via Subdomains

**Identity Resolution** — Extract tenant from subdomain, validate against reserved names.

```rust
// Example: acme-org.fluxbase.dev
// Headers: Host: acme-org.fluxbase.dev
// ↓
// Resolved: tenant_slug="acme-org", tenant_id=<uuid>
```

**Reserved Subdomain Blocking** — Prevents hijacking of platform-critical names:
- Exact matches: `api`, `auth`, `dashboard`, `admin`, `flux`, `fluxbase`, etc.
- Prefix blocks: `api-*`, `auth-*`, `admin-*` (prevent `api-test-org`)
- Response: **421 Misdirected Request** (signals intentional platform claim)

**Slug Normalization**:
- Lowercase
- Strip non-ASCII (blocks homograph attacks, punycode exploits)
- Keep only `[a-z0-9-]`, collapse consecutive dashes
- Validates before tenant lookup

---

### 2. Routing & Function Invocation

**Route Lookup** — In-memory snapshot of all routes (tenant_id, method, path) → function.

After identity resolution → lookup route in snapshot:
```
(tenant_id, "POST", "/api/users/create") 
  ↓ 
RouteRecord { 
  function_id: <uuid>,
  is_async: false,
  auth_type: "jwt",
  cors_enabled: true,
  rate_limit: 100,  // per minute
  jwks_url: "https://...",
  ...
}
```

**CORS Preflight Fast-Path** — If `OPTIONS` + CORS enabled, respond immediately without backend call:
- Headers: `Access-Control-Allow-Origin`, `Access-Control-Allow-Methods`, etc.
- Status: `204 No Content`

**Function Invocation** — Routes to the **Runtime Service** (async execution engine):
1. Resolve route from snapshot
2. Authenticate (JWT or API Key validation)
3. Pass-through request to runtime
4. Forward response to client

**Async Execution** — For `is_async=true` routes, fire job to Queue instead of waiting.

---

### 3. Intelligent Query Caching

**Scope & Eligibility**:
- Only caches **read-only** POST requests to `/db/query`
- Skips large payloads, queries with `offset` or unbounded `limit`, random-order results
- **Role-aware** — JWT `role` claim included in cache key (prevents RLS/CLS leaks)

**Cache Key** — (project_id, role, body_hash):
- Body hash uses **partial SHA256** for speed:
  - First 4 KiB of request body
  - Plus body length as u64 (little-endian)
  - Collision resistance: two queries would need identical prefix + length + differ only beyond 4 KiB (unrealistic with JSON)

**Response Storage** — Zero-copy sharing:
- Body: `Bytes` (Arc<[u8]> internally) — O(1) clone
- Headers: `Arc<HeaderMap>` — pointer bump, not copy
- Sensitive headers stripped before storage (no `set-cookie`, `authorization`, etc.)

**Cache Lifecycle**:
```
1. CACHE HIT
   ├─ Body: clone Bytes pointer (2-4ns)
   ├─ Headers: clone Arc (1-2ns)
   └─ Response: X-Cache: HIT, X-Cache-Age: <ms>

2. CACHE MISS (inflight)
   ├─ Multiple concurrent requests → single backend call (single-flight)
   ├─ All await same Future<Shared>
   └─ Response: X-Cache: MISS

3. CACHE BYPASS
   └─ Non-/db/query, non-cacheable, or invalidated
      Response: X-Cache: BYPASS
```

**Invalidation**:
- **Automatic expiry** — 30s TTL (configurable via `QUERY_CACHE_TTL_SECS`)
- **Background eviction** — Runs every 60s to remove expired entries
- **Manual trigger**:
  ```bash
  POST /internal/cache/invalidate
  Authorization: x-service-token
  {
    "project_id": "...",
    "table": "users"  // optional — if present, only evict entries with table_hint
  }
  ```
- **Table-aware** — Queries can hint the primary table used; invalidation by table name

**Performance Metrics**:
- Cache stats endpoint: `GET /internal/cache/stats` → `{ "entries": N }`
- Slow-query logging: DB queries > 50ms logged to `platform_logs`

---

### 4. Event Streaming (SSE)

**Passthrough Proxy**:
- Client: `GET /events/stream?table=users&conditions=...`
- Gateway: Extract auth headers + Fluxbase scope headers, forward to API
- Response: Transparent SSE stream (events streamed directly to client)

**Benefits**:
- Clients connect to gateway port (8081) instead of API
- Auth tokens cached in gateway JWKS cache
- Load distributed to edge gateway service
- Long-lived HTTP connections don't block unrelated routes

---

### 5. Authentication & Authorization

#### JWT Validation
- Headers: `Authorization: Bearer <jwt>`
- Middleware extracts JWKS URL from route config
- **JWKS Caching** — Fetches and caches public key sets (no per-request fetch)
  - **TTL-based refresh**: Default 5 minutes (`JWKS_CACHE_TTL_SECS=300`)
  - **Key rotation handling**: If verification fails with cached keys, refresh and retry (handles provider key rotation)
  - **Security critical**: JWKS must refresh to detect revoked keys or rotation events
- Validates: signature, audience, issuer
- Extracts `role` claim for query cache isolation

#### API Key Validation
- Headers: `X-API-Key: <key>`
- Looks up in `api_keys` table
- Checks `is_revoked` flag

#### CORS Preflight
- `OPTIONS` requests bypass auth if CORS enabled
- Returns allowed origins, methods, headers

---

### 6. Rate Limiting

**Token Bucket Algorithm**:
- Per-route limit (requests/minute) stored in route config
- Checked per identity (tenant + route)
- Stateful buckets in `RateLimiter` (DashMap)

**Bucket Capacity** (**CRITICAL FIX**):
- Capacity = route rate_limit (e.g., 100 req/min = 100 tokens max)
- Tokens refill at `rate_limit / 60` per second
- Each request costs 1 token
- Burst up to capacity allowed
- **Tokens never accumulate beyond capacity** (no infinite accumulation during idle periods)

**Example**:
- Route rate_limit = 100 req/min
- Capacity = 100 tokens
- Refill rate = 100/60 = 1.666 tokens/sec
- After 1 minute idle: bucket = 100 (capped, not 100+100)
- Burst: can send 100 requests instantly, then wait for refill

**Result**:
- No thundering herd on service restart
- Fair rate limiting even after extended idle time
- Well-defined maximum burst capacity per route
- Excess requests: `429 Too Many Requests`

---

### 7. Distributed Tracing & Observability

**Request ID Propagation**:
- Extract `x-request-id` header (or generate UUID)
- Forward to all downstream services (Runtime, Data-Engine, API)
- Used to correlate logs across services

**Automatic Span Logging** — Fire-and-forget writes to `platform_logs`:
```sql
INSERT INTO platform_logs (id, tenant_id, project_id, source, resource_id, level, message, request_id, span_type)
VALUES (..., 'gateway', <function_id>, 'info', 'route matched: POST /api/users/create', <request_id>, 'start')
```
- Spans for routing, function invocation, DB queries
- `span_type`: `start` | `end` | or custom
- No blocking — spawned as detached tokio task

**Slow Query Logging** — DB queries > 50ms logged at `WARN` level with:
- Request ID
- Table hint
- Filter columns
- Duration

---

## Security Model

### Host Header Validation

The gateway validates all incoming requests to prevent host header injection attacks:

**Policy**:
1. Extract `Host` header (or `X-Forwarded-Host` from load balancer)
2. Validate format: must be `{tenant-slug}.{base-domain}`
3. Base domain must match configured value (e.g., `fluxbase.dev`)
4. Reject non-matching hosts with **400 Bad Request**

**Examples**:
- ✓ `acme-org.fluxbase.dev` — valid
- ✗ `acme-org.attacker.com` — rejected (wrong base domain)
- ✗ `localhost` — rejected (missing tenant slug)

**Implementation**: In `identity_resolver.rs`, Host validation occurs before tenant lookup.

### Internal Service Token Management

The gateway uses `INTERNAL_SERVICE_TOKEN` for internal-only endpoints like `/internal/cache/invalidate`.

**Requirements**:
- **Rotation**: Tokens should be rotated every 60-90 days (implement via secret manager)
- **Scope**: Only for internal service-to-service calls (Data-Engine, API, Runtime)
- **Exposure**: Never expose to client environments (mobile apps, browsers, public SDKs)
- **Validation**: Token is embedded in HTTP header `x-service-token` and validated via exact string match
- **IP Restriction** (recommended): Restrict `/internal/*` endpoints to internal network:
  ```rust
  // Pseudocode
  if path.starts_with("/internal/") {
      // Check X-Forwarded-For or remote_addr against internal CIDR
      if !is_internal_ip(request.ip) {
          return Err(403 Forbidden);
      }
      // Also validate token
      if header("x-service-token") != INTERNAL_SERVICE_TOKEN {
          return Err(401 Unauthorized);
      }
  }
  ```
- **Future Enhancement**: JWT-based service tokens with expiry and scoped claims (e.g., `scope: [cache:invalidate, cache:read]`)

### Runtime Authentication Verification

The gateway forwards `x-request-id` and authentication context to Runtime. **Runtime must verify these headers come from gateway**:

**Verification Policy**:
```
POST /execute
x-request-id: <uuid>
x-service-token: <INTERNAL_SERVICE_TOKEN>  // Runtime validates this is from gateway
x-tenant-id: <uuid>      # UUID of tenant (consistent naming: -id suffix)
x-project-id: <uuid>     # UUID of project (consistent naming: -id suffix)

Runtime actions:
1. Verify x-service-token matches INTERNAL_SERVICE_TOKEN (only gateway has this)
2. Verify tenant_id and project_id are present (would be missing in direct client calls)
3. Reject requests not from gateway IP range
4. Use x-request-id for span parent ID
```

**Why**: Without endpoint verification, an attacker could:
- Hit Runtime directly, bypass gateway auth
- Bypass rate limiting
- Bypass gateway tracing root
- Execute functions with fake tenant/project IDs

---

---

## Operational Readiness

### Health & Readiness Checks

The gateway exposes two health endpoints for load balancers and orchestrators:

**GET /health** (health check):
- Status: `200 OK { "status": "ok" }`
- Indicates: Process is alive
- Latency: <1ms
- Use: Load balancer health probe (fast fail on crash)

**GET /readiness** (readiness check) — *[Future]*:
- Status: `200 OK` if dependencies ready, `503 Service Unavailable` otherwise
- Checks:
  - ✓ PostgreSQL connection pool (ping query)
  - ✓ Snapshot cache loaded (routes in memory) — **MUST be OK before any traffic**
  - ✓ JWKS cache initialized (public key fetch succeeded)
- Latency: 100-500ms (includes network calls)
- Use: Orchestrator readiness probe (prevent traffic before initialization)

#### Startup Behavior & Snapshot Safety

The gateway **must not accept traffic** before the routing snapshot is successfully loaded from the database.

**Recommended Startup Flow**:

```
Gateway Process Start
         ↓
    Initialize logger
         ↓
    Load routing snapshot from database
    (if fails → retry loop or crash)
         ↓
    Initialize caches:
      • JWKS cache (empty initially, lazy-load)
      • Query cache (empty initially)
      • Rate limiter state (empty initially)
         ↓
    Start HTTP server on port 8081
         ↓
    HTTP handlers ready
    (/health → 200 OK immediately)
    (/readiness → 200 OK if snapshot loaded)
         ↓
    Periodic jobs:
      • Snapshot refresh every 60s
      • JWKS TTL refresh
      • Cache eviction
```

**Startup Guarantees**:

- **Before snapshot loads**:
  - `/health` → `200 OK` (process alive, but not ready)
  - `/readiness` → `503 Service Unavailable` (not ready for traffic)
  - All incoming requests → `503 Service Unavailable` with error: `Gateway snapshot not loaded`

- **After snapshot loads**:
  - `/health` → `200 OK`
  - `/readiness` → `200 OK`
  - Function requests → normal processing

**Why This Matters**:

Without startup sequencing, early client requests could receive:
- `404 Not Found` (route not in empty snapshot)
- Cascading errors if upstream services retry immediately
- Confusing traces with gateway as failure point

With startup ordering:
- Orchestrator sees `503` during initialization
- Load balancer removes gateway from rotation
- Client requests queue at LB, not at gateway
- First client request processed after snapshot is guaranteed to succeed or fail for correct reason

**Implementation Note**: Use a startup sync primitive (`tokio::sync::Notify` or atomic bool) to block HTTP handler execution until snapshot is ready.

### Timeout Policy

All upstream service calls enforce strict timeouts to prevent gateway hangs:

| Service | Timeout | Reason |
|---------|---------|--------|
| Runtime | 30s | Serverless function execution time |
| Data-Engine | 15s | Structured query + validation |
| Queue | 5s | Job submission (fire-and-forget) |
| API (SSE) | None (idle-only) | **Long-lived streams; don't kill valid connections** |
| JWKS Fetch | 10s | Public key provider availability |
| Database Queries | 5s | Local network operations |

**SSE Timeout Detail**: Server-Sent Events are intentionally long-lived (hours). The gateway does NOT apply a fixed timeout. Instead:
- **Idle timeout** (future): Disconnect if no event sent for 30 minutes
- **Activity**: Keep-alive comments prevent idle disconnects
- **Client reconnect**: Browser automatically reconnects on drop

**Circuit Breaker** (Future Enhancement):

If Runtime / Data-Engine repeatedly timeout or return 5xx, the gateway should "open the circuit" and fail fast:

```rust
// Pseudocode: Circuit breaker state machine
enum CircuitState {
    Closed,           // Normal operation
    Open,             // Fail fast without trying
    HalfOpen,         // Retry once per timeout interval
}

// Example: Open circuit after 5 consecutive failures
if consecutive_timeouts >= 5 {
    circuit_state = Open;
    timer::after(30s).then(|| circuit_state = HalfOpen);
}

// When circuit is Open:
// POST /proxy → 503 Service Unavailable immediately (no attempt)
```

Benefits:
- Prevents cascading failures (downstream doesn't get DDoS'd with retries)
- Fast fail allows client to retry or use fallback
- Automatic recovery after cooldown period

**Backpressure Handling**: If Runtime is slow and requests queue up, the gateway will hit timeouts before connection pools exhaust. This is preferable to silent hangs. A circuit breaker (when implemented) prevents hammering unhealthy services.

### Trace Sampling Strategy

Logging every request to `platform_logs` will cause table bloat. The gateway implements selective sampling:

**Policy** (configurable via `TRACE_SAMPLING_*` env vars):

- **100% sampling for**:
  - HTTP 5xx responses (errors)
  - Requests > 200ms (slow requests)
  - Requests with auth_failed or rate_limit_exceeded
  - Specific tenants (debug mode)

- **Configurable sampling for successful requests**:
  - Default: 10% (1 in 10 successful requests)
  - Tunable per tenant (premium tenants get 100%)
  - Can be disabled for high-volume read workloads

**Why**: Without sampling, a busy platform logs:
```
100 req/sec × 5 spans per request × 86,400 sec/day
= 43.2 million rows per day
```

Sampling reduces to ~4 million rows with error/slow request coverage intact.

### Tracing & Log Write Failure Handling

Trace spans are written asynchronously and **must never block request execution**:

**Guarantee**: Logging failures are silent and non-blocking.

```rust
// Fire-and-forget span logging
tokio::spawn(async move {
    let result = sqlx::query(...)
        .execute(&pool)
        .await;
    
    if let Err(e) = result {
        // Log to stderr, but don't propagate
        tracing::warn!("Failed to write span: {}", e);
    }
});

// Continue request immediately
```

**Why**: If the database is slow or unreachable, function execution must not pause. Trace integrity is secondary to request latency.

**Monitoring**: If DB connection pool is exhausted, spans pile up in tokio task queue. Alert if pending spans > 1000.

#### Backpressure Protection (Future Enhancement)

Under extreme conditions (DB unavailable for extended time), span logging tasks may accumulate unboundedly in the tokio task queue, consuming memory and potentially crashing the gateway.

**Recommended Design**: Introduce a **bounded span queue**.

```rust
// Pseudocode: Architecture
const MAX_PENDING_SPANS: usize = 5000;

let (tx, rx) = tokio::sync::mpsc::channel(MAX_PENDING_SPANS);

// Gateway request handler
tokio::spawn(async move {
    match tx.try_send(span) {
        Ok(_) => {}, // Span enqueued
        Err(mpsc::error::TrySendError::Full(_)) => {
            // Queue is full; drop span
            tracing::warn!("Dropped span due to queue saturation (DB unavailable?)");
        }
        Err(mpsc::error::TrySendError::Closed(_)) => {
            // Receiver dropped; application is shutting down
        }
    }
});

// Span writer worker
while let Some(span) = rx.recv().await {
    if let Err(e) = db.insert_span(span).await {
        tracing::warn!("Failed to write span: {}", e);
    }
}
```

**Behavior**:
- **Normal**: Spans enqueued and written at ~1000s/sec
- **DB slow**: Queue fills up; new spans dropped
- **Alert triggered**: Gateway operator alerted to investigate

**Benefits**:
- Prevents unbounded memory growth during DB outages
- Maintains non-blocking logging guarantee
- Protects gateway stability under failure conditions
- Clear observability (dropped span count metric)

**Trade-off**: Some spans lost during sustained DB failures (acceptable; operational alerts take precedence over perfect trace capture)

### Built-in Metrics & Observability

The gateway **must** expose real-time metrics to operators. Current implementation:

**Required Metrics** (tracked in-memory, exported via `/metrics` in future):

| Metric | Type | Purpose |
|--------|------|---------|
| `gateway_requests_total` | Counter | Total requests by route, method, status |
| `gateway_request_duration_seconds` | Histogram | Request latency p50/p95/p99 per route |
| `gateway_cache_hits_total` | Counter | Query cache hits by project |
| `gateway_cache_misses_total` | Counter | Query cache misses by project |
| `gateway_cache_size_bytes` | Gauge | Current query cache memory usage |
| `gateway_rate_limit_rejections_total` | Counter | Rejected requests by tenant, route |
| `gateway_auth_failures_total` | Counter | Auth failures (JWT, API key) by type |
| `gateway_jwks_refresh_total` | Counter | JWKS refreshes by provider |
| `gateway_db_connections_open` | Gauge | Active DB connections in pool |
| `gateway_db_write_errors_total` | Counter | Failed platform_logs writes |
| `gateway_snapshot_refresh_seconds` | Histogram | Time to refresh routing snapshot |

**Prometheus Integration** (Future):
```
GET /metrics
# HELP gateway_requests_total Total HTTP requests
# TYPE gateway_requests_total counter
gateway_requests_total{route="/api/users",method="POST",status="200"} 1024
```

**Why**: Without metrics, operating Fluxbase is blind. When p95 latency rises, you need to know if it's:
- Slow auth (JWKS timeout)?
- Cache misses (thundering herd)?
- DB pool exhaustion?
- Slow downstream service?

**Current Status**: Metrics infrastructure not yet implemented; high priority for production.

---



## Request Flow

### Serverless Function Invocation

```
1. Client Request
   POST /api/users/create
   Host: acme-org.fluxbase.dev
   Authorization: Bearer <jwt>
   
2. Gateway Identity Resolver
   ├─ Extract subdomain from Host: "acme-org"
   ├─ Normalize & validate: "acme-org" (check reserved list)
   ├─ Lookup tenant_id from snapshot
   └─ Attach ResolvedIdentity to request extensions

3. Router Selection (based on path + method)
   ├─ Lookup in snapshot: (tenant_id, "POST", "/api/users/create")
   └─ If hit → continue; if miss → 404 Not Found

4. Authentication
   ├─ Route auth_type = "jwt"
   ├─ Extract bearer token from Authorization header
   ├─ Validate signature + audience using cached JWKS
   └─ Extract role claim

5. CORS Preflight Check
   ├─ If OPTIONS + cors_enabled → 204 No Content + CORS headers
   └─ Otherwise → continue

6. Rate Limiting
   ├─ Check tokens for (tenant_id, route_id)
   ├─ If limit exceeded → 429 Too Many Requests
   └─ Otherwise → consume 1 token, continue

7. Tracer Span (async, non-blocking)
   └─ Insert "route matched" span to platform_logs

8. Request Forwarding to Runtime
   ├─ Forward headers: auth, x-request-id, x-tenant-id, x-project-id
   ├─ Forward body as-is
   ├─ If is_async=true → redirect to Queue (fire job)
   └─ Otherwise → wait for response

9. Response Passthrough
   ├─ Status + Headers from upstream
   └─ Body streamed directly to client
```

### Data Engine Query (with Caching)

```
1. Client Request
   POST /db/query
   Host: acme-org.fluxbase.dev
   Authorization: Bearer <jwt>
   Content-Type: application/json
   {"table": "users", "where": {"id": "123"}}

2. Identity Resolution (same as above)

3. CORS Preflight (same as above)

4. Cacheability Check
   ├─ Method = POST ✓
   ├─ Path ends with "/db/query" ✓
   ├─ Body not too large ✓
   ├─ No "offset" in request ✓
   ├─ Limit bounded (not null or > threshold) ✓
   └─ Not random-ordered → CACHEABLE

5. Cache Key Generation
   ├─ project_id from headers
   ├─ role extracted from JWT
   ├─ body_hash = SHA256(body[..4096] + len(body).to_le_bytes())
   └─ key = (project_id, role, body_hash)

6. Cache Lookup
   ├─ Hit → jump to step 9 (Hit Response)
   └─ Miss → jump to step 7 (Inflight Check)

7. Inflight Concurrency Check
   ├─ Is another request with same key in flight?
   ├─ Yes → attach to shared Future, await with others
   └─ No → proceed to step 8

8. Backend Call (single-flight)
   ├─ Forward request to Data-Engine URL
   ├─ Include x-service-token, x-request-id
   ├─ Await response → (status, headers, body)
   ├─ Strip sensitive headers (set-cookie, authorization, etc.)
   ├─ Wrap body in Bytes, headers in Arc<HeaderMap>
   └─ Store in cache, notify waiting requests

9. Hit Response (or result from step 8)
   ├─ Status: from cache
   ├─ Headers: clone Arc (O(1))
   ├─ Body: clone Bytes pointer (O(1))
   ├─ Add X-Cache: HIT | MISS | BYPASS
   ├─ Add X-Cache-Age: <elapsed_ms>
   └─ Return to client

10. Background Tasks (non-blocking)
    ├─ Log query span to platform_logs
    ├─ Log slow query if duration > 50ms
    └─ Check & evict expired cache entries (every 60s)
```

### Cache Invalidation

```
Scenario: User updates a record in the "users" table

1. Data-Engine processes write mutation
   └─ Detects table affected: "users"

2. Data-Engine calls Gateway
   POST /internal/cache/invalidate
   x-service-token: <internal_service_token>
   {"project_id": "...", "table": "users"}

3. Gateway Invalidation Logic
   ├─ Validate service token
   ├─ Iterate all cache entries with project_id
   ├─ For each entry with table_hint="users" → remove
   └─ Return: { "ok": true, "evicted": 42, "remaining": 128 }

Result: 
  • All cached queries on "users" table cleared
  • Next query on "users" table → backend call (MISS)
  • Queries on other tables remain cached
```

---

## Configuration

### Environment Variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `DATABASE_URL` | Required | PostgreSQL connection string (Neon) |
| `RUNTIME_URL` | `http://localhost:3001` | Serverless execution engine |
| `QUEUE_URL` | `http://localhost:8083` | Async job queue service |
| `DATA_ENGINE_URL` | `http://localhost:8082` | Structured query + file ops |
| `GATEWAY_PORT` or `PORT` | `8081` | HTTP listen port |
| `INTERNAL_SERVICE_TOKEN` | Required | Secret for internal endpoints (`/internal/*`) |
| `API_URL` | `http://localhost:8080` | Fluxbase API (for SSE proxy) |
| `QUERY_CACHE_TTL_SECS` | `30` | Cache entry lifetime in seconds |
| `QUERY_CACHE_MAX_ENTRIES` | `4096` | Maximum cached query responses |
| `QUERY_CACHE_MAX_RESPONSE_SIZE` | `1MB` | **Max response size to cache** (prevents bloat; responses > 1MB bypass cache) |
| `JWKS_CACHE_TTL_SECS` | `300` | JWKS refresh interval in seconds (5 minutes) |
| `PLATFORM_LOGS_POOL_SIZE` | `20` | connection pool size for trace writes |
| `TRACE_SAMPLING_ERROR_RATE` | `1.0` | Always sample errors / 500s (1.0 = 100%) |
| `TRACE_SAMPLING_SLOW_THRESHOLD_MS` | `200` | Sample requests slower than this |
| `TRACE_SAMPLING_SUCCESS_RATE` | `0.1` | Sample successful requests (0.1 = 10%) |

### Database Schema

Key tables referenced:

**tenants**
```sql
id UUID PRIMARY KEY
slug VARCHAR UNIQUE
```

**projects**
```sql
id UUID PRIMARY KEY
tenant_id UUID REFERENCES tenants(id)
```

**routes**
```sql
id UUID PRIMARY KEY
project_id UUID REFERENCES projects(id)
path VARCHAR
method VARCHAR
function_id UUID
is_async BOOLEAN
auth_type VARCHAR ('none', 'api_key', 'jwt')
cors_enabled BOOLEAN
rate_limit INTEGER
jwks_url VARCHAR
jwt_audience VARCHAR
jwt_issuer VARCHAR
json_schema JSONB
cors_origins TEXT[]
cors_headers TEXT[]
```

**platform_logs**
```sql
id UUID PRIMARY KEY
parent_span_id UUID NULL  -- Enables real trace tree hierarchy
request_id UUID NOT NULL  -- Enforced UUID type; prevents spoofing
tenant_id UUID NOT NULL
project_id UUID NOT NULL
source VARCHAR (255) NOT NULL  -- 'gateway', 'runtime', 'data-engine', etc.
span_type VARCHAR (64) NOT NULL  -- 'start', 'complete', etc.
level VARCHAR (32) DEFAULT 'info'  -- 'info', 'warn', 'error'
message TEXT
created_at TIMESTAMP NOT NULL DEFAULT NOW()

-- Indexes for query patterns
INDEX (request_id, created_at)  -- Trace reconstruction
INDEX (parent_span_id)          -- Child lookup
INDEX (tenant_id, created_at)   -- Multi-tenant isolation
```

**api_keys**
```sql
id UUID PRIMARY KEY
project_id UUID
key_hash VARCHAR
is_revoked BOOLEAN
created_at TIMESTAMP
```

---

## API Endpoints

### Public Routes

#### Health Check
```
GET /health
Response: { "status": "ok" }
```

#### Version Info
```
GET /version
Response: {
  "service": "gateway",
  "commit": "<git_sha>",
  "build_time": "<timestamp>"
}
```

#### Serverless Function Invocation
```
{ANY} /{path}
Prerequisites:
  - Subdomain resolves to valid tenant
  - Route exists for (tenant_id, method, path)
  - Client authenticated (JWT or API Key)
  - Rate limit not exceeded

Response:
  - Status, headers, body from Runtime service
  - X-Request-ID: correlation ID
```

#### Database Query Execution
```
POST /db/query
Prerequisites:
  - Valid project scope (X-Fluxbase-Project header)
  - Read-only query

Response:
  - Query result from Data-Engine
  - X-Cache: HIT | MISS | BYPASS
  - X-Cache-Age: <milliseconds>
```

#### File Operations
```
{GET|PUT|DELETE|POST} /files/{path}
Prerequisites:
  - Valid project scope
  - Authenticated

Response:
  - File operation result from Data-Engine
```

#### Real-time Events (SSE)
```
GET /events/stream?table={table}&conditions={conditions}
Prerequisites:
  - Valid auth headers (Authorization / X-API-Key)
  - Fluxbase scope headers (X-Fluxbase-Tenant, X-Fluxbase-Project)

Response:
  - Content-Type: text/event-stream
  - Real-time table change events
  - Connection: keep-alive (long-lived)
```

### Internal Routes (Service-Token Protected)

#### Cache Invalidation
```
POST /internal/cache/invalidate
Headers: x-service-token: <internal_service_token>
Body: {
  "project_id": "<uuid>",
  "table": "<table_name>"  // optional
}

Response: {
  "ok": true,
  "evicted": <count>,
  "remaining": <count>
}
```

#### Cache Statistics
```
GET /internal/cache/stats
Response: {
  "entries": <count>
}
```

---

## Deployment

### Docker Build

```dockerfile
# Build stage: Compile Rust with SQLx offline mode
FROM rust:1.93-bookworm AS builder

WORKDIR /usr/src/app
COPY . .

ENV SQLX_OFFLINE=true
RUN cargo build --release -p gateway

# Runtime stage: Minimal image with binary
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y libssl-dev ca-certificates

COPY --from=builder /usr/src/app/target/release/gateway /usr/local/bin/

ENV PORT=8080
EXPOSE 8080

CMD ["./gateway"]
```

### Cloud Run Deployment

```bash
# Build & push to artifact registry
make deploy-gcp SERVICE=gateway

# Sets environment variables from env.yaml
# Maps port 8081 → 8080 internally
# Enables traffic to latest revision
```

### Predeployment Checks

```bash
# Verify configuration
echo "DATABASE_URL: ${DATABASE_URL}"
echo "RUNTIME_URL: ${RUNTIME_URL}"
echo "DATA_ENGINE_URL: ${DATA_ENGINE_URL}"

# Verify routing snapshot loads
curl http://localhost:8081/health
# Expected: {"status": "ok"}
```

---

## Performance Characteristics

### Latency Budget (for 95th percentile)

| Component | Latency | Notes |
|-----------|---------|-------|
| Identity resolution | 1ms | Substring from Host + HashSet lookup |
| Route lookup (snapshot) | 0.1ms | HashMap with (tenant_id, method, path) |
| CORS preflight | 1ms | No backend call, headers only |
| JWT validation | 5-10ms | JWKS cached; ~100-200µs for verification |
| Auth (API Key) | 2-5ms | Single DB roundtrip |
| Query cache lookup | 0.5ms | DashMap hit |
| Data-Engine proxy | 50-200ms | Network round-trip + backend processing |
| Logging (async) | 0ms | Fire-and-forget, non-blocking |

**Cache Hit Path**: ~5ms end-to-end (identity + route + cache + CORS)
**Cache Miss Path**: ~60-220ms (network-bound to Data-Engine)

### Memory Usage

| Component | Footprint |
|-----------|-----------|
| Routing snapshot | ~30 MB (5000 routes × ~6 KB metadata) |
| Query cache (4096 entries, capped) | ~400 MB (100 KB avg per entry, **with max_cached_response_size enforcement**) |
| JWKS cache (10 providers, TTL 5min) | ~500 KB |
| Per-request allocations | Minimal (stack-only for most fields) |
| **Total (typical)** | **~500-600 MB** |

**Snapshot Estimate Breakdown**: Each RouteRecord contains:
- id, path, method, function_id (80B)
- auth_type, is_async, rate_limit (40B)
- jwks_url, jwt_audience, jwt_issuer (200B)
- json_schema JSONB (1-2KB typical)
- cors_origins, cors_headers arrays (2-3KB)
- **Total ~6KB per route** (not 2KB as previously estimated)

### Concurrency Model

- **Tokio async runtime** — M:N threading, efficient for I/O
- **Single-flight deduplication** — Shared futures prevent thundering herd
- **Zero-copy response sharing** — Arc pointers, no data duplication
- **Non-blocking logging** — platform_logs inserts spawned as detached tasks

---

## State Mutations: Event-Sourced Observability (Git for Backend State)

The revolutionary piece that enables true time-travel debugging for entire backend systems:

**Record every state mutation as an append-only event linked to request_id.**

This automatically transforms Fluxbase into an event-sourced system, without forcing developers to design around it.

### Schema: State Mutation Log

Add a new table:

```sql
state_mutations
id UUID PRIMARY KEY
request_id UUID              -- links to the request that caused this change
span_id UUID NULL            -- if from nested execution
tenant_id UUID

entity_type TEXT             -- "table:users", "table:orders", "file", "queue:jobs"
entity_id TEXT               -- primary key / identifier (e.g., "user-123")

operation TEXT               -- insert | update | delete | append

before JSONB NULL            -- state before mutation (null for insert)
after JSONB NULL             -- state after mutation (null for delete)

created_at TIMESTAMP

INDEX (request_id, created_at)
INDEX (entity_type, entity_id, created_at)  -- query entity history
INDEX (created_at)  -- time-based queries
```

### Example: State Mutations from a User Signup Request

Request: `POST /signup` with `request_id = 550e8400-...`

Every state change is recorded:

```sql
Mutation 1: Insert User
  request_id: 550e8400-...
  entity_type: table:users
  entity_id: user-123
  operation: insert
  before: null
  after: {
    "id": "user-123",
    "email": "alice@example.com",
    "name": "Alice",
    "created_at": "2026-03-09T15:30:45Z"
  }

Mutation 2: Send Email
  request_id: 550e8400-...
  entity_type: tool:gmail
  entity_id: message-887
  operation: insert
  before: null
  after: {
    "id": "message-887",
    "to": "alice@example.com",
    "status": "sent",
    "timestamp": "2026-03-09T15:30:46Z"
  }

Mutation 3: Queue Job
  request_id: 550e8400-...
  entity_type: queue:jobs
  entity_id: job-901
  operation: insert
  before: null
  after: {
    "id": "job-901",
    "function": "send_welcome_email",
    "status": "pending",
    "scheduled_at": "2026-03-09T15:30:47Z"
  }
```

Now the request is a **complete state transition** — not just execution trace, but what changed.

### State Timeline: Reconstructing the Backend at Any Point

With append-only mutations linked to request_id, you can reconstruct backend state exactly as it was at any moment:

```
Timeline of state changes:

T1: 2026-03-09T15:30:00  request A creates user-123 (Alice)
T2: 2026-03-09T15:31:15  request B updates user-123 (Alice → Alice Smith)
T3: 2026-03-09T15:32:42  request C deletes user-123

State reconstruction:

at T1:
  users[user-123] = {name: "Alice"}

at T2:
  users[user-123] = {name: "Alice Smith"}

at T3:
  users[user-123] = deleted
```

**Implementation:** To reconstruct state at time T:
1. Find latest snapshot before T (or start empty)
2. Replay all mutations with `created_at <= T` in order
3. Apply each (insert/update/delete) to get final state

### New CLI Commands: Time-Travel for Backend State

**Inspect backend state at a specific time:**

```bash
$ flux state inspect --at 2026-03-09T15:00:00

Database state @ 2026-03-09T15:00:00

users
  user-1    Alice Smith
  user-2    Bob Johnson

orders
  order-101 {user: user-1, amount: $45.99}
  order-102 {user: user-2, amount: $120.00}

queue_jobs
  (empty)
```

**See exactly what a request changed:**

```bash
$ flux trace state 550e8400--e29b-41d4-a716-446655440000

State changes from request

users
  + user-123 {name: "Alice", email: "alice@example.com"}

email_messages
  + message-887 {to: "alice@example.com", status: "sent"}

queue_jobs
  + job-901 {function: "send_welcome_email", status: "pending"}
```

**Rewind system to before a request executed:**

```bash
$ flux state checkout 550e8400

Reverting system to state before request 550e8400...

Changes reverted:

users[-] user-123
email_messages[-] message-887
queue_jobs[-] job-901

System restored to 2026-03-09T15:30:00 (before request)
```

**View complete state history of an entity:**

```bash
$ flux state history table:users:user-123

History of users[user-123]

T1: 2026-03-09T15:30:45  INSERT {name: "Alice"}
    request:550e8400
    code_sha:a93f42c

T2: 2026-03-09T16:00:12  UPDATE {name: "Alice Smith"}
    request:7c2b9d1
    code_sha:b8f51d9

T3: 2026-03-09T16:15:33  DELETE
    request:e4a3c2d
    code_sha:a93f42c
```

### Storage Strategy

The mutation log is append-only and grows linearly:

**Scale estimate:**

```
100M requests/day
~5 mutations per request (on average)
= 500M mutations/day
~ 1 KB per mutation (JSON overhead)
≈ 500 GB/day

Mitigation strategies:

1. Sampling:
   - 100% sampling for mutations affecting critical entities
   - 10% sampling for routine CRUD
   - Can reduce to ~50 GB/day

2. Snapshots:
   - Periodic snapshot every 10 minutes
   - Allows pruning old mutations
   - Replay only mutations after latest snapshot

3. Compression:
   - Archive old mutations
   - gzip compressed mutations ~80% reduction
   - Keep 30 days hot, 90 days archived

4. Entity-level TTL:
   - Keep mutations for entity X for 90 days
   - Older mutations can be pruned if snapshot exists
```

### How to Implement: Runtime State Tracking

When a function executes and modifies state, the runtime automatically logs mutations:

```javascript
// Example: Runtime automatically tracks state changes

// User signup function
async function create_user(req) {
  // Database mutation
  const user = await ctx.db.insert("users", {
    email: req.email,
    name: req.name
  });
  
  // Runtime automatically logs:
  // state_mutations {
  //   entity_type: "table:users",
  //   entity_id: user.id,
  //   operation: "insert",
  //   before: null,
  //   after: {email, name}
  // }
  
  // Tool mutation
  await ctx.tool.gmail.send({
    to: req.email,
    subject: "Welcome"
  });
  
  // Runtime automatically logs:
  // state_mutations {
  //   entity_type: "tool:gmail",
  //   entity_id: <message_id>,
  //   operation: "insert",
  //   after: {to, subject, status: "sent"}
  // }
}
```

Developer writes normal code. Runtime intercepts state changes and logs them.

### Handling Nondeterminism in Replay

**Q: How do you handle randomness and generated IDs when replaying?**

**A: Record the outputs, replay uses recorded values.**

Example:

```
Original execution:

request A calls uuid()
generated: user-123

Mutation log records:
  entity_id: user-123  ← the actual generated value

Replay:

flux trace replay <request-A>
↓
uuid() called
↓
Runtime intercepts and injects recorded value: user-123
↓
Deterministic replay executed with same generated values
```

This is the same approach used in deterministic replay debuggers (e.g., rr).

### External Tool Calls During Replay

Tool calls are handled by classification:

| Tool Type | Original | Dry-Run | Replay with `--enable-real-tools` |
|-----------|----------|---------|-------------------------------------|
| `db.insert` | Real | Mocked | Real (idempotent required) |
| `db.update` | Real | Mocked | Real (idempotent required) |
| `db.delete` | Real | Mocked | Real (idempotent required) |
| `gmail.send` | Real | Mocked | Mocked (financial: never double-send) |
| `stripe.charge` | Real | Mocked | Mocked (always prevents double-charge) |
| `workflow.run` | Real | Mocked | Mocked (would create duplicate jobs) |

### The Complete Feature Stack

Your system now supports:

| Command | What | Dimension |
|---------|------|-----------|
| `flux trace <id>` | View execution trace | Execution |
| `flux trace debug <id>` | Step through past execution | Execution + State |
| `flux trace replay <id>` | Re-execute request | Execution |
| `flux trace diff <a> <b>` | Compare traces | Execution |
| `flux trace blame <id>` | Git blame for code | Code |
| `flux trace state <id>` | What changed | State |
| `flux state inspect --at T` | Backend at time T | State |
| `flux state history <entity>` | Entity change history | State |
| `flux state checkout T` | Restore system state | State |

This is:

- **Git** (blame, history, checkout, diff)
- **+ Debugger** (step through execution, inspect locals)
- **+ APM** (traces, metrics, performance)
- **+ Database time machine** (reconstruct state at any time)

For backend execution.

### Why This Is Unique

Traditional platforms capture:
- Span duration
- Error messages
- Custom attributes

Event-sourced systems capture:
- State changes
- But require developer design

Fluxbase captures both automatically:
1. **Execution** (trace + checkpoints) — what happened
2. **State mutations** (append-only log) — what changed
3. **Code provenance** (code_sha) — which code caused it
4. **Request envelope** (trace_requests) — what was the input

Developers don't change their code. The platform transparently records everything.

### The Ultimate Debugging Experience

Engineer gets bug report: "User balance is wrong"

```bash
# Find affected user
$ flux state history table:users:user-123

Shows all mutations to that user

# See a suspicious update
$ flux trace blame <suspicious-request-id>

Shows which code change caused it

# Understand the issue
$ flux trace debug <request-id>

Step through execution, inspect variables at time of change

# Verify fix
$ flux trace replay <request-id> --function-version <new-version>

Compare old vs new behavior

# Restore if needed
$ flux state checkout <request-id>

Rewind system to before the bug
```

All without leaving the command line. All without reproduction. All with complete audit trail.

---

One of the killer features enabled by the gateway architecture:

```
RouteRecord now could include:
  version_id: <uuid>
  version_weight: 10  // Send 10% traffic to this version

Result:
  GET /api/users/create
    ├─ 90% → function-version-v1 (stable)
    └─ 10% → function-version-v2 (canary, new code)

Tracing automatically shows:
  • Which version was invoked
  • Latency diff between versions
  • Error rates by version
  • Cost by version

Rollout workflow:
  1. Deploy new function version
  2. Set weight=1% in route config
  3. Monitor traces for errors/latency
  4. Gradually increase weight (1% → 5% → 50% → 100%)
  5. Lock in weight=100% when confident
```

This is enabled **entirely at the gateway** — no code changes needed, no function redeployment.

---

## Time-Travel Debugging: Replay & Trace Diffing

Fluxbase's trace architecture enables a unique capability: **replay any past request** and **compare execution traces** — similar to `git log` and `git diff` but for backend execution.

This transforms debugging from "what happened?" to "what would happen if I run it again?" and "why did this change?"

### The Single Architectural Change: Request Envelope Capture

To enable time-travel debugging, the gateway captures the **canonical request envelope** when a request arrives:

**New Table: `trace_requests`**

```sql
trace_requests
id UUID PRIMARY KEY
request_id UUID UNIQUE  -- links to platform_logs
tenant_id UUID
project_id UUID

-- Original request snapshot
method VARCHAR(10)  -- GET, POST, etc.
path TEXT
headers JSONB
query_params JSONB
body JSONB

-- Execution context
function_id UUID
function_version TEXT  -- e.g., "v7" or "a93f42c"
created_at TIMESTAMP

-- Optional artifact reference (for large payloads)
artifact_uri TEXT NULL  -- S3://, gs://, etc. for >10MB bodies

-- Sampling & TTL
INDEX (request_id, created_at)  -- lookups and time ordering
INDEX (tenant_id, created_at)   -- tenant audit
INDEX (replay_of)               -- finding related replays
```

**Trace Requests Sampling Policy**

Capturing every request envelope will cause table bloat. Apply selective sampling:

**Policy** (configurable via `TRACE_ENVELOPE_SAMPLING_*` env vars):

- **100% sampling for**:
  - HTTP 5xx responses (errors)
  - Requests > 200ms (slow requests)
  - Requests with auth_failed or rate_limit_exceeded
  - Requests from debug tenants (opt-in debug mode)

- **Configurable sampling for successful requests**:
  - Default: 10% (1 in 10 successful requests)
  - Tunable per tenant (premium tenants get 100%)
  - Can be disabled for high-volume read tenants

**Why**: At scale, storage without sampling explodes:
```
100M req/day × 5 KB/request × 30 days retention
= ~152 TB for full history
```

With 10% sampling (plus 100% for errors/slow):
```
100M req/day × 90% sampling (10%) × 5 KB/request × 30 days
≈ 13.5 TB (acceptable, indexable)
```

**Implementation**:

```rust
fn should_sample_envelope(response_status: u16, duration_ms: u64, tenant_id: &UUID) -> bool {
    // Always sample errors and slow requests
    if response_status >= 500 || duration_ms > 200 {
        return true;
    }
    
    // Check tenant debug mode
    if DEBUG_TENANTS.contains(tenant_id) {
        return true;
    }
    
    // Sample success with configurable rate
    rand::random::<f64>() < TRACE_ENVELOPE_SAMPLING_RATE  // 0.10 default
}
```

**Update to `platform_logs`:**

Add two fields:
```sql
replay_of UUID NULL               -- if non-NULL, this trace is a replay of another trace
code_sha TEXT NULL                -- commit SHA / bundle hash of deployed function
```

**Why this model:**
- **Single source of truth** — captured once at the gateway, before any mutation
- **Storage efficient** — avoids payload duplication across spans
- **Complete context** — contains everything needed to reconstruct execution
- **Replay-compatible** — original request can be replayed deterministically
- **Code provenance** — every span linked to exact code version (enables git-level debugging)

---

### Code Provenance: Linking Traces to Git

The most powerful addition to time-travel debugging is **code provenance** — recording which exact commit/bundle version executed each span.

**Schema Update:**

Add to `platform_logs`:
```sql
code_sha TEXT NULL  -- commit SHA or bundle hash
INDEX (code_sha, created_at)  -- query all executions of a specific code version
```

**How it works:**

When a function is deployed:

```
1. Bundle code: bundle = tar(function files)
2. Hash it: code_sha = sha256(bundle)
3. Store in function_versions table:
   
   function_versions
   ├─ id UUID
   ├─ function_id UUID
   ├─ code_sha TEXT  -- a93f42c (truncated SHA)
   ├─ commit_message TEXT  -- from git
   ├─ git_commit_sha TEXT  -- full commit hash
   ├─ author TEXT
   ├─ created_at TIMESTAMP
   └─ s3_bundle_uri TEXT  -- where to fetch exact code
```

When runtime executes:

```
1. Runtime receives request
2. Function metadata includes code_sha
3. Executes with: x-function-sha: a93f42c
4. Gateway logs code_sha to every span:
   
   platform_logs
   ├─ request_id = 550e8400-...
   ├─ function_id = acme-create-user
   ├─ code_sha = a93f42c  ← linked to git commit
   ├─ span_type = complete
   └─ duration_ms = 145
```

**This enables Git-level debugging:**

| Capability | What it answers | How |
|------------|-----------------|-----|
| **Blame** | What code caused this? | `flux trace blame <id>` → git commit + diff |
| **Regression Detection** | Which version broke this? | Compare latency across code_sha values |
| **Auto-Rollback** | Should we revert this version? | If error_rate(code_sha) > threshold → trigger rollback |
| **Perfect Replay** | Which exact bundle to run? | `flux trace replay <id>` loads s3://bundles/a93f42c.tar |
| **Bisect** | Which commit introduced the bug? | `flux trace bisect <function>` runs git bisect on code_sha |

---

### Git Blame for Production: `flux trace blame`

**The killer CLI command:**

```bash
$ flux trace blame 550e8400-e29b-41d4-a716-446655440000

Execution failed

Function: create_user
Code version: a93f42c

Commit:
  SHA: a93f42c123456789...
  Author: shashi
  Date: 2026-03-09 14:32
  Message: add stricter email validation (RFC 5322)

git diff HEAD~1..a93f42c:

  - if (!email.includes("@")) return false;
  + if (!EMAIL_REGEX.test(email)) return false;

Suggestion:
  Regression detected in email validation logic
  Previous version: a82d91a (avg: 0.2% errors)
  This version: a93f42c (avg: 8.1% errors)
  
  Recommendation: rollback to a82d91a
```

**Workflow:**

1. Customer reports: "Signup broken"
2. Engineer: `flux trace blame <failed-request-id>`
3. Instantly see: commit + diff + metrics
4. Root cause visible in code diff
5. Decision: rollback or fix forward

---

### Regression Detection by Code Version

```bash
$ flux trace stats create_user --by code_sha

Version Latency Analysis

a93f42c  (current)
  •avg: 1200ms ⚠
  requests: 45,230
  errors: 3,640 (8.1%)
  p95: 2100ms

a82d91a  (previous)
  • avg: 145ms ✓
  requests: 120,400
  errors: 24 (0.02%)
  p95: 280ms

Regression detected: +1055ms avg latency
Rollback recommended
```

---

### Automatic Rollback on High Error Rate

With code provenance, the platform can automatically roll back broken deployments:

```rust
// Policy: if error_rate for specific code_sha > threshold
fn should_rollback(code_sha: &str) -> bool {
    let error_rate = query_error_rate(code_sha, last_1_hour);
    let prev_error_rate = query_error_rate(previous_code_sha, last_7_days);
    
    // If error rate jumped >5% vs previous stable version
    if (error_rate - prev_error_rate) > 0.05 {
        return true;  // Trigger auto-rollback
    }
    false
}
```

Example:

```
create_user v8 (b9f42a) deployed 10 minutes ago
error_rate_v8: 14%
error_rate_v7: 0.2%

Δ = +13.8% → triggers auto-rollback to v7
```

---

---

### Code-Level Debugging: Execution Checkpoints

Beyond code blame and regression detection, execution checkpoints enable something remarkable: **inspecting local variable state at any point in a past execution**, without re-running code.

**The key insight:** Location alone is not enough. You need execution state.

**What code_location alone gives you:**

```
create_user.ts:12
```

What it answers: Where did the code run?

**What execution_state gives you:**

```json
{
  "code_location": "create_user.ts:12:validate_email",
  "checkpoint_type": "branch",
  "execution_state": {
    "locals": {
      "email": "alice@",
      "regex": "RFC5322",
      "isValid": false
    }
  }
}
```

What it answers: **What were the values when the code executed?**

That's the difference between knowing a function failed and understanding **why** it failed.

---

### Schema: Execution Checkpoints

Add to `platform_logs`:

```sql
code_sha TEXT NULL                    -- commit/bundle hash
code_location TEXT NULL               -- file.ts:line:function
checkpoint_type TEXT NULL             -- function_entry, branch, tool_call, db_query, error, return
execution_state JSONB NULL            -- local variables + branch decisions + state
```

Example trace with checkpoints:

```sql
platform_logs entries for request 550e8400:

Entry 1: gateway.receive
  source: gateway
  span_type: start
  request_id: 550e8400-...

Entry 2: runtime.execute_function
  source: runtime
  code_sha: a93f42c
  checkpoint_type: function_entry
  code_location: create_user.ts:5
  execution_state: {
    "locals": {
      "input": { "name": "Alice", "email": "alice@" }
    }
  }

Entry 3: runtime.checkpoint
  source: runtime
  code_sha: a93f42c
  checkpoint_type: branch
  code_location: create_user.ts:12
  execution_state: {
    "locals": {
      "email": "alice@",
      "regex": "RFC5322",
      "isValid": false
    },
    "branch_taken": "email_validation_failed"
  }

Entry 4: runtime.error
  source: runtime
  code_sha: a93f42c
  checkpoint_type: error
  code_location: create_user.ts:13
  execution_state: {
    "error": "ValidationError",
    "locals": {
      "message": "Invalid email format"
    }
  }
```

---

### The Time-Travel Debugger CLI: `flux trace debug`

With execution checkpoints, the CLI becomes a full debugger for past execution:

```bash
$ flux trace debug 550e8400

Debugging request 550e8400
Function: create_user (v a93f42c)

Frame 1: function_entry
  Location: create_user.ts:5
  Locals:
    input.name = "Alice"
    input.email = "alice@"

Frame 2: branch
  Location: create_user.ts:12
  Code: if (!EMAIL_REGEX.test(email))
  Condition: true (email invalid)
  Locals:
    email = "alice@"
    regex = "RFC5322"
    isValid = false

Frame 3: error
  Location: create_user.ts:13
  Code: throw new ValidationError(...)
  Error: ValidationError("Invalid email format")

Suggestion:
  The regex requires complete email format.
  Previous version a82d91a accepted partial emails.
  
  Recommendation:
    $ git show a93f42c --  create_user.ts | grep EMAIL_REGEX
```

**Key advantage over traditional debugging:**

No reproduction needed. No staging environment. No local setup. Just inspect the exact state that existed in production at that moment.

---

### Storage Efficiency

The overhead is minimal because:

1. **Checkpoints are selective** — Not every line, only logical decisions:
   - Function entry/return
   - Branch decisions  
   - External tool calls
   - Database queries
   - Error states

2. **Typical function:**
   ```
   create_user()
   ├─ checkpoint: function_entry (5 local variables)
   ├─ checkpoint: validate_email branch (3 variables)
   ├─ checkpoint: db.insert call (2 variables)
   ├─ checkpoint: gmail.send branch (1 variable)
   └─ checkpoint: return (1 variable)
   
   Total: ~5-10 checkpoints per function
   Payload: 2-5 KB per request
   ```

3. **Sampling strategy:**
   With 100% sampling for errors + slow requests (>200ms) + 10% success rate:
   ```
   100M req/day × 10% sample × 5 KB extra per request
   ≈ 50 GB/day (negligible vs 152 TB without sampling)
   ```

---

### Checkpoint Types

Standard checkpoint types for UI categorization:

| Type | When | Captures |
|------|------|----------|
| `function_entry` | Function called | Input parameters |
| `branch` | If/switch decision | Condition, branch taken |
| `tool_call` | External API called | Tool name, arguments |
| `db_query` | Database operation | Query, parameters |
| `workflow_invoke` | Nested function call | Called function, arguments |
| `error` | Exception thrown | Error type, message, locals |
| `return` | Function returns | Return value, locals |

---

### Runtime Implementation

In runtime, emit checkpoints at key moments:

```javascript
// Function entry
ctx.__trace.checkpoint({
  checkpoint_type: "function_entry",
  code_location: "create_user.ts:5",
  locals: { input }
});

// Branch decision
if (!EMAIL_REGEX.test(email)) {
  ctx.__trace.checkpoint({
    checkpoint_type: "branch",
    code_location: "create_user.ts:12",
    locals: { email, regex: "RFC5322", isValid: false },
    branch_taken: "email_validation_failed"
  });
  
  throw new ValidationError("Invalid email");
}

// Tool call
ctx.__trace.checkpoint({
  checkpoint_type: "tool_call",
  code_location: "create_user.ts:20",
  locals: { email },
  tool: "gmail.send"
});
await ctx.tool.gmail.send_email({ to: email });

// Return
ctx.__trace.checkpoint({
  checkpoint_type: "return",
  code_location: "create_user.ts:35",
  locals: { result }
});
return result;
```

Each checkpoint is stored as a platform_logs entry with `execution_state JSONB`.

---

### The Complete Feature Stack

Your system now supports:

| Command | What it does | Mode |
|---------|--------------|------|
| `flux trace <id>` | View complete trace tree | Read-only |
| `flux trace replay <id>` | Re-execute with same inputs | Execute |
| `flux trace diff <a> <b>` | Compare two traces | Read-only |
| `flux trace blame <id>` | Link to git commit + diff | Read-only |
| `flux trace debug <id>` | Step through past execution | Read-only |

This is:
- **Git** (blame, history, diff)
- **+ Debugger** (step, inspect locals, breakpoints/checkpoints)
- **+ APM** (traces, metrics, performance)

For backend execution.

---

### Why This Is Unique

Traditional platforms (Datadog, Honeycomb, New Relic) capture:
- Span duration
- Error messages
- Custom attributes

But they cannot:
- Inspect local variable state
- See execution branching logic
- Replay with same inputs
- Link to source code commits

Fluxbase can do all of these because:

1. You control the **gateway** (entry point, creates tracing root)
2. You control the **runtime** (can instrument checkpoint emission)
3. You control the **database schema** (can store execution_state)
4. You own **both** code and execution (can link code_sha to git commits)

No other serverless platform has this combination.

---

**CLI Command: Replay a Request**

```bash
$ flux trace replay 550e8400-e29b-41d4-a716-446655440000

Replaying request...
gateway.receive
├─ gateway.auth_passed                 4ms
├─ gateway.rate_limit_passed           2ms
└─ runtime.execute_function        0-92ms
   ├─ data_engine.db.insert       15-52ms
   ├─ composio.gmail.send_email   38-95ms
   └─ workflow.invoke             12-45ms

Original duration:  145ms
Replay duration:     92ms

New trace_id: f9e1234a-d5c2-4a77-9c01-1a2b3c4d5e6f
```

**Execution Flow:**

1. Query `trace_requests` for original envelope
2. Extract: method, path, headers, body, function_version
3. Reconstruct HTTP request
4. Send to gateway with header: `X-Replay-Of: <original-request-id>`
5. Gateway processes normally (auth, rate limiting, tracing)
6. Routes to Runtime with same context
7. New trace created with `replay_of = original-request-id`

### Safe Replay Modes

**Replay Authentication Policy**

JWTs expire. When replaying after token expiry, the gateway must decide how to authenticate:

- **Mode 1: Preserve Original Auth (Default)**
  ```bash
  $ flux trace replay <id>
  ```
  - Use the original JWT from `trace_requests.headers`
  - If expired: Returns 401 Unauthorized (replay fails)
  - Best for: Exact reproduction of original execution
  - Risk: Fails if original token is no longer valid

- **Mode 2: Bypass Auth (Internal Debugging)**
  ```bash
  $ flux trace replay <id> --bypass-auth
  ```
  - Skip JWT validation
  - Inject `X-Bypass-Auth: internal-debug` header
  - Requires internal service token verification
  - Best for: Incident investigation when original token lost
  - Risk: Executes with admin privileges regardless of original role

- **Mode 3: Replace with Operator Token (Recommended)**
  ```bash
  $ flux trace replay <id> --operator-token <token>
  ```
  - Replace original JWT with operator's token
  - Preserves role claims from operator (not original request)
  - Best for: Safe replay with audit trail
  - Risk: Results may differ if authorization context affects behavior

**Recommendation**: Default to Mode 1 (fail if expired). Operators must explicitly request Mode 2/3 for historical replays.

**Queue Replay Safety**

Replaying a request that originally called external tools carries risk:

```
Original trace:
  └─ stripe.charge($100)      ✓ charged
  └─ send_email("order confirmed")

IfReplayed without precautions:
  └─ stripe.charge($100)      ✓ CHARGED AGAIN (duplicate charge!)
  └─ send_email("order confirmed")  ✓ EMAIL SENT AGAIN
```

**Default Behavior**: External tool calls are **mocked by default** unless explicitly enabled.

```bash
$ flux trace replay <id>                               # Safe (mocked tools)
$ flux trace replay <id> --enable-real-tools           # Dangerous (real calls)
```

Tool visibility:

| Tool | Default | Notes |
|------|---------|-------|
| `stripe.charge` | MOCK | Always mock (financial risk) |
| `stripe.refund` | MOCK | Always mock (financial risk) |
| `gmail.send_email` | MOCK | Mock by default (spam risk) |
| `slack.post_message` | MOCK | Mock by default |
| `db.insert` | REAL | Idempotency required (see below) |
| `db.update` | REAL | Idempotency required |

**Idempotency Requirement**:

For database writes to be safe during replay, functions must be idempotent:

```javascript
// ✗ NOT IDEMPOTENT: creates duplicate rows
ctx.db.insert("orders", { order_id: req.id, amount: 100 });

// ✓ IDEMPOTENT: upsert (insert or update if exists)
ctx.db.upsert("orders", 
  { order_id: req.id }, 
  { order_id: req.id, amount: 100 }
);
```

**Dry-Run Mode** (Safe for Side Effects)

```bash
$ flux trace replay <id> --dry-run

Behavior:
  • All tool calls mocked
  • Database writes skipped
  • Execution logic runs (compute, validation, etc.)
  • Observability: logs what WOULD have happened

Mocked tools:
  composio.gmail.send_email → MOCK (no email sent)
  stripe.charge → MOCK (no charge)
  
Skipped writes:
  INSERT users → SKIPPED
  UPDATE orders → SKIPPED

Safe for:
  • Testing incident causes
  • Verifying fixes
  • Debugging without side effects
```

Implementation: Gateway injects `X-Replay-Mode: dry-run` header; Runtime/Data-Engine mock external calls.

**Partial Replay** (Execute from Specific Span)

```bash
$ flux trace replay <id> --from data_engine.db.insert

Skips:
  • gateway.receive
  • gateway.auth
  • runtime.execute_function start

Starts at:
  • data_engine.db.insert (with original query)

Useful for:
  • Isolating failures to specific service
  • Testing fixes in downstream systems
  • Performance regression in one service
```

**Version-Pinned Replay**

```bash
$ flux trace replay <id> --function-version v5

Re-executes with function version v5 (original was v7).

Compares:
  • v7 latency vs v5 latency
  • Behavior changes between versions
  • Regression detection
```

### Trace Diffing

**Compare Two Traces**

```bash
$ flux trace diff <orig-id> <new-id>

runtime.execute_function
  orig: 145ms
  new:   98ms
  Δ: -47ms (-32%)

├─ data_engine.db.insert
    orig: 50ms
    new:  22ms
    Δ: -28ms (-56%)
    
└─ composio.gmail.send_email
    orig: 95ms
    new:  76ms
    Δ: -19ms (-20%)

New code is 32% faster overall.
Attribute: new index on users.email_hash (reduces query from 50ms to 22ms)
```

**What Differs Reports:**
- Span latencies (original vs replay)
- Error rates
- Span count changes
- Tool call outputs
- Cache hit/miss patterns
- Resource attribution

**Use Cases:**
- **Regression detection** — did this deployment slow things down?
- **Fix validation** — did my code change improve latency?
- **Performance attribution** — which deployment caused the slowdown?
- **Behavioral changes** — why does my function output differ?

### Use Cases: Real-World Scenarios

**1. Customer Incident Forensics**

Customer reports: "My order failed yesterday at 3 PM."

Operator workflow:

```bash
# Find the trace
$ flux trace search order_id=12345 --time "2026-03-09T15:00:00Z"

Found: 550e8400-e29b-41d4-a716-446655440000

# Replay the exact request
$ flux trace replay 550e8400-e29b-41d4-a716-446655440000 --dry-run

# See what happens now
gateway.receive
├─ auth_passed
├─ rate_limit_passed
└─ runtime.execute_function [NOW SUCCEEDS]

# The failure was Stripe timeout (now resolved)
# Confirm with diff
$ flux trace diff 550e8400-e29b-41d4-a716-446655440000 <new-replay-id>

stripe.charge latency: 15000ms → 450ms (API recovered)
```

**2. Deployment Regression Detection**

After deploying function `create_user` v8:

```bash
# Compare v7 (stable) vs v8 (new)
$ flux trace search function=create_user limit=100 | head -10 | xargs -I{} flux trace diff {} <v8-trace-id>

Results:
  v7 avg latency: 145ms
  v8 avg latency: 1200ms

Regression detected! 8.2x slower.

# Rollback v8, investigate
$ git log --oneline v7..v8
  a93f42c: add full-text search index on bio field
  
# The new index query is slow. Revert and optimize.
```

**3. A/B Testing Validation**

Testing two versions of a checkout flow:

```bash
$ flux trace diff <checkout-v1-id> <checkout-v2-id>

v1: creates order → charges stripe → sends email
v2: creates order → sends email → charges stripe (reordered)

latency impact:
  v1: 200ms
  v2: 180ms (-10%)

behavior:
  v1: email sent after charge
  v2: email sent before charge (order still pending, riskier UX)

Decision: v1 is safer, accept latency cost.
```

### Storage & Sampling Considerations

**Storage Cost:**

Typical request envelope:
- Headers: ~1 KB
- Query params: ~100 B
- Body: 1-5 KB

Total: ~2-6 KB per request

At 100M requests/day:
- Raw storage: ~200-600 GB/day
- With compression (gzip): ~30-90 GB/day
- Acceptable on cloud storage (S3, GCS)

**TTL Policy:**

Recommend: 30 days retention for `trace_requests` (configurable)

```
trace_requests TTL: 30 days (sufficient for incident investigation)
platform_logs TTL: 7 days (shorter for span details, traces are queryable via envelope)
```

**Large Payload Handling:**

For bodies > 10 MB:
- Store in object storage (S3, GCS)
- Reference via URI in `artifact_uri` field
- Fetch on replay if needed

### Schema Integration

Complete request envelope flow:

```
Client Request (POST /api/users/create, body={...})
         ↓
Gateway.receive
  ├─ INSERT platform_logs (span: start)
  ├─ INSERT trace_requests (canonical envelope)  ← captured once
  ├─ INSERT platform_logs (span: route_matched)
  ├─ INSERT platform_logs (span: auth_passed)
  └─ Forward to Runtime
         ↓
Runtime.execute_function
  ├─ INSERT platform_logs (span with parent_span_id)
  ├─ Call db.insert
  └─ INSERT platform_logs (span: complete)
         ↓
Later: flux trace replay <request-id>
  ├─ SELECT * FROM trace_requests WHERE request_id = ?
  ├─ Reconstruct request
  ├─ Send to gateway (with X-Replay-Of header)
  └─ INSERT platform_logs with replay_of = original_request_id
```

### Example: Time-Travel UI

Dashboard shows:

```
Request: create_user (order_id=12345)
Time: 2026-03-09 15:23:45 UTC

Trace: 550e8400-...

Spans:
  gateway.receive
  gateway.auth_passed (6ms)
  runtime.execute_function
    ├─ data_engine.db.insert (50ms)
    ├─ composio.gmail.send_email (95ms)  [ERROR: timeout]
    └─ [FAILED]

[Replay] [Diff] [Partial Replay]

Click [Replay] → Re-executes now → Success (stripe recovered)
Click [Diff] → Shows: email_send latency 95ms → 40ms (3x faster now)
```

---

## Incident-Level Deterministic Replay

**The killer feature: reproduce production incidents in an isolated sandbox.**

This is the capstone of the entire architecture. Because Fluxbase captures request envelopes, execution traces, code versions, state mutations, and execution checkpoints, it can replay an entire production incident deterministically — including all concurrency, timing, and state interactions.

### The Command

```bash
flux incident replay 2026-03-09T14:00..14:05
```

This recreates the entire 5-minute incident in a sandbox, with production state and traffic.

### How It Works

#### Phase 1: Extract Incident Traffic

All requests in the time window are extracted from `trace_requests`, in original order:

```sql
SELECT *
FROM trace_requests
WHERE created_at BETWEEN '2026-03-09T14:00:00Z' AND '2026-03-09T14:05:00Z'
ORDER BY created_at ASC
```

Example:

```
14:00:01.234 POST /v1/checkout body: {user_id: 123, items: [...], total: 99.99}
14:00:02.801 GET /api/status
14:00:03.456 POST /v1/checkout body: {user_id: 456, items: [...], total: 149.99}
14:00:04.712 POST /webhooks/stripe body: {event: payment_failed, ...}
14:00:07.089 POST /v1/checkout body: {user_id: 789, items: [...], total: 199.99}
...
```

You now have the exact production traffic: sequence, timing, and payloads.

#### Phase 2: Create Isolated Sandbox

Fluxbase launches a dedicated sandbox environment:

```bash
Sandbox: incident-replay-9382-v1
  Created: 2026-03-09T16:30:00Z
  Status: provisioning...
  
  Components:
  ├─ Gateway (same version & config)
  ├─ Runtime (same version & config)
  ├─ Data Engine (same version & config)
  ├─ Queue (same version & config)
  ├─ Database snapshot (isolated)
  └─ Tool mocks (email, payments, webhooks)
```

**Database State Restoration:**

The sandbox database is restored to the exact state 1 second before the incident:

```bash
# Reconstruct state at 2026-03-09T13:59:59Z
flux state snapshot --at 2026-03-09T13:59:59Z --sandbox incident-replay-9382-v1
```

This works because `state_mutations` is append-only and linked to requests:

```
[Snapshot at 13:59:59] → [Replay mutations 13:59:59 - 14:00:00] → [Backend state = production at 14:00:00]
```

Now the sandbox backend is **identical to production before the incident occurred**.

#### Phase 3: Replay Captured Requests

Requests are replayed in exact order. Two modes:

**Mode 1: Deterministic Replay**

Original timing preserved, reproducing concurrency bugs:

```bash
flux incident replay 2026-03-09T14:00..14:05 --mode deterministic
```

Timeline:

```
14:00:01.234 → POST /v1/checkout (original delay from 14:00:00)
14:00:02.801 → GET /api/status  (1.567s after first)
14:00:03.456 → POST /v1/checkout (0.655s after second)
14:00:04.712 → POST /webhooks/stripe (1.256s after third)
...
```

Each request waits for the original inter-arrival delay, reproducing timing-sensitive race conditions.

**Mode 2: Accelerated Replay**

All requests executed immediately:

```bash
flux incident replay 2026-03-09T14:00..14:05 --mode accelerated
```

For rapid debugging when timing is not the issue.

### During Replay: Live Inspection

While replay is running or after it completes, you can:

**View Traces**

```bash
# Compare side-by-side: original vs replay
flux trace live --sandbox incident-replay-9382-v1

# Shows real-time spans as requests execute
```

**Diff Traces**

```bash
# Original vs replay: exact latency/error diffs
flux trace diff --original 550e8400-... --replay 550e8401-...

# Output:
# Span: runtime.execute_function
#   Original: 145ms (succeeded)
#   Replay:   156ms (succeeded)
#   Δ: +11ms
#
# Span: stripe.payment_charge
#   Original: 2500ms (ERROR: timeout)
#   Replay:   1200ms (succeeded)
#   Δ: -1300ms (FIXED!)
```

**Step Through Execution**

```bash
# Debug a specific checkout request
flux trace debug --sandbox incident-replay-9382-v1 --request-id 550e8401-...

# Interactive debugger:
# (flux-debug) l          # local variables
# (flux-debug) n          # step next
# (flux-debug) s          # step into tool call
# (flux-debug) c          # continue to next error
```

**Inspect State Changes**

```bash
# What did each request mutate?
flux state history --sandbox incident-replay-9382-v1 --entity table:orders

# Output:
# 14:00:01 order_id: 12345 status: pending → processing
# 14:00:02 order_id: 12346 status: pending → processing
# 14:00:03 order_id: 12347 status: pending → failed (stripe timeout)
# ...
```

### Partial Incident Replay

Not every request matters. Filter to the relevant subset:

**By Route**

```bash
flux incident replay 2026-03-09T14:00..14:05 --filter route=/v1/checkout
```

Only checkout requests replayed.

**By Tenant**

```bash
flux incident replay 2026-03-09T14:00..14:05 --filter tenant=acme-org
```

Only requests from tenant `acme-org`.

**By User**

```bash
flux incident replay 2026-03-09T14:00..14:05 --filter user_id=456
```

Only requests from user 456.

**By Status**

```bash
flux incident replay 2026-03-09T14:00..14:05 --filter status=error
```

Only failed requests — useful for focused debugging.

### External System Handling

During replay, external systems are handled safely:

| System | Production | Sandbox |
|--------|-----------|---------|
| Database writes | Production DB | Sandbox DB (isolated) |
| Queue jobs | Production queue | Sandbox queue (isolated) |
| Email sends | Gmail / SendGrid | Mocked (logged) |
| Payment charges | Stripe / PayPal | Mocked (logged) |
| Webhooks | External hosts | Mocked (logged) |
| API calls | Live services | Mocked (logged) |

This prevents accidental:

- Double-charging customers
- Sending duplicate emails
- Calling external APIs twice
- Mutating external state

All tool outputs are **recorded from the original request** and **replayed deterministically**.

Example:

```
Original request 14:00:01:
  stripe.charge({customer_id: 123, amount: 99.99})
  → Result: {charge_id: ch_1234, status: succeeded}

Sandbox replay 14:00:01:
  stripe.charge({customer_id: 123, amount: 99.99})
  → Uses recorded result: {charge_id: ch_1234, status: succeeded}
  → No actual charge made
```

### Testing Fixes: The Killer Workflow

Engineer receives incident report:

```
Checkout failures 14:00-14:05
  Error: stripe.charge timeout (2500ms threshold exceeded)
  Impact: 127 failed checkouts, $8,500 revenue lost
```

**Step 1: Start Replay (1 second)**

```bash
flux incident replay 2026-03-09T14:00..14:05
```

Sandbox starts. Incident reproduces instantly.

**Step 2: Deploy Patched Code (5 seconds)**

```bash
git checkout fix/stripe-timeout
cargo build -p runtime --release
flux deploy --sandbox incident-replay-9382-v1 --version patched
```

Patched function deployed to sandbox.

**Step 3: Re-run Replay (5 seconds)**

```bash
flux incident replay 2026-03-09T14:00..14:05 --sandbox incident-replay-9382-v1
```

Same 127 requests, same timing, same state — but with patched code.

**Step 4: Compare Results (2 seconds)**

```bash
flux trace compare --original incident-replay-9382-v1-orig --patched incident-replay-9382-v1-patched

# Output:
# Total requests: 127
# Errors before:  127 (100%)  [all timeout: stripe.charge]
# Errors after:   0   (0%)
# 
# Latency improvement:
#   stripe.charge: 2500ms → 850ms
#   
# Revenue recovered: $8,500
# 
# ✓ FIX VALIDATED
```

Engineer now has **proof** the fix works — without affecting production.

### Alternate Timeline Simulation

You can also test what would have happened with different code:

```bash
flux incident replay 2026-03-09T14:00..14:05 \
  --sandbox incident-replay-9382-original \
  --function-version production

flux incident replay 2026-03-09T14:00..14:05 \
  --sandbox incident-replay-9382-patched \
  --function-version fix/stripe-timeout

flux trace compare --original incident-replay-9382-original --patched incident-replay-9382-patched
```

Result: **Branching backend histories** — you can see exactly how different code versions would have behaved.

### Why This is Technically Feasible

Deterministic replay requires three things:

| Requirement | How Fluxbase Provides It |
|------------|--------------------------|
| **Input** | `trace_requests` (request envelope: method, path, headers, body, query, tenant, project) |
| **Execution** | `platform_logs` (complete trace with spans, timings, errors, tool calls) |
| **State** | `state_mutations` (append-only log of all backend mutations) |
| **Code** | `code_sha` (exact deployed version) |
| **Checkpoints** | `execution_state` (local variables, branches, decisions) |

This is the same principle used in:

- AFL (American Fuzzy Lop) — records crashing inputs, replays deterministcally
- Perses — record-and-replay debugging (Mozilla)
- `rr` (Record & Replay) — Linux kernel-level debugging
- Temporal / Durable Execution — replay workflow steps

Fluxbase applies this pattern to **entire backend systems**.

### Why Other Platforms Can't Do This

Most platforms store only logs:

```
AWS Lambda:
  ✗ No request envelope
  ✗ No state mutations
  ✗ No execution checkpoints
  ✓ Only: log lines, duration, error message

Vercel:
  ✗ No request envelope
  ✗ No state mutations
  ✗ No execution checkpoints
  ✓ Only: error message, duration

Cloudflare Workers:
  ✗ No request envelope
  ✗ No state mutations
  ✗ No execution checkpoints
  ✓ Only: log lines

Temporal:
  ✓ Stores workflow state
  ✗ No database state mutations (app-dependent)
  ✗ No network request envelopes
  ✗ No execution checkpoints

Fluxbase:
  ✓ Request envelopes (trace_requests)
  ✓ State mutations (state_mutations)
  ✓ Code version (code_sha)
  ✓ Execution checkpoints (execution_state)
  ✓ Complete traces (platform_logs)
  → Deterministic replay enabled
```

### Storage Requirements

Storing incident replay data doesn't significantly increase storage:

- `trace_requests`: Already captured at 100% error rate, ~10% success rate
- `state_mutations`: Append-only, ~50GB/day at scale (with sampling + snapshots)
- `platform_logs`: Spans with checkpoints, ~13.5GB/day at scale
- `execution_state`: Captured at logical checkpoints only (~2-5KB overhead)

**Total: ~75GB/day for complete incident replay capability at 100M requests/day.**

Archived incidents older than 90 days can be compressed (~80% reduction) or deleted per retention policy.

### The Backend Time-Travel Stack

This transforms Fluxbase into the ultimate debugging platform:

```
Git CLI                 Fluxbase Equivalent
──────────————————────────────────────────
git log                 flux trace
git blame               flux trace blame
git show                flux trace debug (with execution_state)
git diff                flux trace diff
git checkout            flux state checkout
git log --follow        flux state history
git bisect              flux incident bisect ← next feature
git revert              (manual rollback, but with replay validation)
```

The workflow:

```
1. Bug detected in production
2. flux incident replay <time> → reproduce locally
3. git log / flux trace blame → identify suspect commits
4. Patch code → test in replay
5. flux trace diff → quantify improvement
6. Deploy → monitor
```

No staging environment needed. No data export required. No reproduction steps needed. **The production incident itself becomes the test case.**

---

## Improvements & Future Considerations
````

### Current Limitations

1. **JWKS Caching** — No explicit refresh; relies on TTL-less caching. Consider:
   - Add `x-jwks-cache-ttl` response header parsing
   - Proactive refresh for critical keys
   
2. **Query Cache Granularity** — Table-aware invalidation is basic:
   - Consider per-column invalidation for fine-grained reads
   - Support compound invalidation rules (e.g., "users + user_roles")

3. **Rate Limiting** — Token bucket is per-route, not tiered:
   - Could add project-level limits (burst across routes)
   - Could add IP-based limits for DDoS mitigation

4. **Snapshot Refresh** — Fixed 60s interval:
   - Add event-driven refresh (listen for route changes in DB)
   - Support hot-reload without server restart

5. **Error Handling** — Some edge cases not fully covered:
   - Upstream 5xx responses could bypass cache (prevent serving stale errors)
   - Partial body read failures silently treated as miss (add metric)
   - Missing validation of `json_schema` in route (add pre-execution validation)

6. **Memory Management** — Current footprint~500-600MB:
   - Fine for Cloud Run / EC2
   - Could become heavy if deployed to edge nodes
   - Consider memory-aware cache eviction policies

### Potential Enhancements

1. **Request Validation** — Pre-validate JSON against route `json_schema` before accepting (fail-fast of invalid input)
2. **Response Transformation** — Per-route header injection, status code mapping, body transformation
3. **Request Signing** — Sign outgoing requests with project key for inter-service auditability
4. **Traffic Splitting** — Canary deployments (v1: 90%, v2: 10% by route version)
5. **Trace Envelope Sampling** — Selective capture into `trace_requests` (100% errors/slow, configurable success rate)
6. **Metrics Export** — Prometheus `/metrics` endpoint for real-time p50/p95/p99 latencies per route
7. **Webhook Retry** — Built-in retry logic with exponential backoff and jitter for webhook failures
8. **Request Deduplication** — Idempotency key support (prevent duplicate writes on automatic retries)
9. **Custom Header Injection** — Per-route custom headers (e.g., X-Fluxbase-Tenant-Tier for multi-tiered SLAs)
10. **Readiness Probe** — Full dependency health check (DB, snapshot, JWKS) on GET /readiness
11. **Event-Driven Snapshot Refresh** — Listen to platform_logs for route mutations instead of pure polling
12. **Request Rate Bucketing** — Per-tenant and per-IP rate limits in addition to per-route
13. **Trace Replay** — Re-execute a request from a stored trace
    ```bash
    # Given a past request_id, replay the exact same flow
    $ flux trace replay <request-id>
    
    # Re-execute function with same:
    # - path + method
    # - request body
    # - tenant_id + project_id
    # - JWT/API key
    #
    # Outputs new trace_id to compare behavior:
    # Original trace: 550e8400-e29b-41d4-a716-446655440000 [145ms] ✓
    # Replayed trace: 550e8400-e29b-41d4-a716-446655460001 [98ms]  ✓ (faster!)
    ```
    
    This enables debugging, regression testing, and incident reconstruction without access to production data.


---

## Testing

### Unit Tests (Caching)

```bash
# Test cache key generation with partial-body hash
# Test sensitivity header stripping
# Test cache invalidation by project + table

cargo test -p gateway cache::
```

### Integration Tests

```bash
# Test end-to-end request flow
# Test identity resolution with various hostnames
# Test JWT validation with mock JWKS
# Test single-flight concurrency

cargo test -p gateway --test integration
```

### Load Testing

```bash
# Benchmark cache hit latency
wrk -t4 -c100 -d30s \
  -s script.lua \
  http://acme-org.fluxbase.dev:8081/db/query

# Expected: >1000 req/s on cache hits
# Expected: <100ms p95 latency
```

---

## Code Organization

```
gateway/
├── src/
│   ├── main.rs             # Entry point, server startup
│   ├── config.rs           # Env var loading
│   ├── state.rs            # Shared state struct
│   ├── router.rs           # Route registration + middleware stack
│   ├── routes/
│   │   ├── mod.rs
│   │   ├── proxy.rs        # Serverless function invocation
│   │   ├── data_engine.rs  # DB query + caching
│   │   ├── events.rs       # SSE passthrough proxy
│   │   └── cache.rs        # Cache invalidation + stats
│   ├── middleware/
│   │   ├── mod.rs
│   │   ├── identity_resolver.rs  # Tenant from subdomain
│   │   ├── jwt_auth.rs           # JWT validation
│   │   ├── rate_limit.rs         # Token bucket
│   │   ├── analytics.rs          # Metrics logging
│   │   └── auth.rs               # API key validation
│   ├── cache/
│   │   ├── mod.rs
│   │   ├── snapshot.rs     # Route cache + bg refresh
│   │   ├── query_cache.rs  # Data query cache + single-flight
│   │   └── jwks.rs         # JWT public key cache
│   ├── services/
│   │   ├── mod.rs
│   │   └── route_lookup.rs # RouteRecord definition
│   └── bin/
│       ├── seed.rs         # Initialize test data
│       └── migrate.rs      # Run migrations
├── Cargo.toml
├── Dockerfile
├── env.yaml                # Default config
└── README.md
```

---

## Summary

The **Gateway** is the **edge runtime orchestrator** and primary control plane entry point for Fluxbase. It implements a complete request lifecycle:

**Request Lifecycle**:
```
Client Request
  ↓
[Gateway]
  ├─ Host header validation
  ├─ Identity resolution (subdomain → tenant)
  ├─ Route lookup (memory snapshot, O(1))
  ├─ Request ID governance (prevent spoofing)
  ├─ Authentication (JWT + JWKS cache or API key)
  ├─ Rate limiting (token bucket)
  └─ Trace root creation (platform_logs INSERT start)
  ↓
[Route Handler]
  ├─ Function Proxy ──→ Runtime (sync) or Queue (async)
  ├─ Data-Engine Proxy ──→ with edge cache + single-flight
  └─ Events Proxy ──→ SSE subscription relay
  ↓
[Response]
  └─ Trace span completion (platform_logs INSERT complete)
```

**Core Responsibilities**:

✅ **Observability by Construction** — Creates trace roots with request_id; spans flow through all services  
✅ **Unified Routing** — Single source of truth for tenant → function mapping (memory snapshot)  
✅ **Fail-Fast Security** — Auth, rate limiting, validation before runtime execution  
✅ **Intelligent Caching** — Single-flight dedup, role-aware isolation, zero-copy sharing  
✅ **Request Coordination** — Routes with full context (tenant, project, user, role, request_id)  
✅ **Event Streaming** — Transparent SSE proxying for real-time subscriptions  
✅ **Performance** — Sub-5ms latency on cache hits, O(1) route lookups, strict timeouts  
✅ **Operational Integrity** — Health checks, timeout policies, sampling, non-blocking logs  

**Key Design Decisions**:
- **Trace roots are authoritative** — Gateway generates request_id; all downstream services use the same ID
- **Snapshot caching is O(1)** — Routes loaded in memory every 60s (future: event-driven)
- **Logging never blocks** — Spans written via `tokio::spawn`; failures are silent
- **Single-flight cache prevents storms** — Identical queries coalesce into one backend call
- **Role-aware caching prevents leaks** — Cache key includes JWT role claim
- **Timeouts are strict** — Runtime: 30s, Data-Engine: 15s, Queue: 5s (prevent hangs)

Fluxbase gateways are perfect for building **trace-first, observable serverless platforms** where every request is automatically woven into a distributed trace tree and sampling prevents log bloat at scale.

Perfect for building **trace-first, observable serverless platforms** where every request is automatically woven into a distributed trace.
