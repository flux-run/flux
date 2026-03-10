# API Service — Architecture & Reference

> **Service name:** `fluxbase-api`  
> **Role:** Control Plane — the single HTTP surface used by the dashboard, CLI, and external API consumers  
> **Tech:** Rust · Axum · SQLx · PostgreSQL (Neon) · S3/R2 (Cloudflare R2 or MinIO)  
> **Default port:** `8080`

---

## Table of Contents

1. [Overview](#1-overview)
2. [Control Plane vs Execution Plane](#2-control-plane-vs-execution-plane)
3. [Module Structure](#3-module-structure)
4. [AppState](#4-appstate)
5. [Middleware Stack](#5-middleware-stack)
6. [Scope & Authorization Model](#6-scope--authorization-model)
7. [Route Groups](#7-route-groups)
   - 7.1 [Platform Routes](#71-platform-routes)
   - 7.2 [Tenant Routes](#72-tenant-routes)
   - 7.3 [Project Routes](#73-project-routes)
   - 7.4 [Internal Routes](#74-internal-routes)
   - 7.5 [Public Routes](#75-public-routes)
8. [Authentication](#8-authentication)
   - 8.1 [Firebase JWT](#81-firebase-jwt)
   - 8.2 [API Keys (`flux_*`)](#82-api-keys-flux_)
   - 8.3 [Internal Service Token](#83-internal-service-token)
9. [Functions](#9-functions)
10. [Deployments](#10-deployments)
11. [Secrets](#11-secrets)
12. [API Keys Management](#12-api-keys-management)
13. [Schema Graph & SDK](#13-schema-graph--sdk)
14. [Gateway Routes](#14-gateway-routes)
15. [Tools & Integrations](#15-tools--integrations)
16. [SSE Events](#16-sse-events)
17. [Logs & Observability](#17-logs--observability)
18. [Data Engine Proxy](#18-data-engine-proxy)
19. [Storage](#19-storage)
20. [CORS & Security](#20-cors--security)
21. [Environment Variables](#21-environment-variables)
22. [Known Issues & Improvement Areas](#22-known-issues--improvement-areas)

---

## 1. Overview

The API service is the **control plane** for Fluxbase. Every action on the platform
(create a function, deploy code, manage secrets, configure gateway routes, browse logs)
passes through this service. It is intentionally **not** the hot-path execution layer — that
belongs to the Gateway and Runtime services.

```
Dashboard / CLI / SDK
        │
        ▼
   ┌──────────┐   auth   ┌──────────────┐
   │ API :8080│◄────────►│ Firebase     │
   └──────────┘          │ Auth / JWKs  │
        │                └──────────────┘
        ├── CRUD ──────► PostgreSQL (Neon)
        ├── proxy ─────► Data Engine :8082
        ├── bundles ───► R2 / S3
        └── logs ───────► R2 / S3  (archive)
```

Key responsibilities:

| Responsibility                | Notes                                                                    |
|-------------------------------|--------------------------------------------------------------------------|
| Identity & multi-tenancy      | Users → Tenants → Projects hierarchy enforced via middleware             |
| Function registry             | Metadata + JSON schema; code stored in R2                               |
| Versioned deployments         | CLI multipart upload, bundle stored in R2, fallback inline in Postgres  |
| Secret management             | AES-256-GCM encrypted, versioned, scoped to tenant or project           |
| API key management            | `flux_*` prefixed, SHA-256 hashed, never stored in plaintext            |
| Schema graph                  | Proxied from Data Engine; used for SDK + OpenAPI generation             |
| Gateway route config          | CRUD for path pattern → function mappings consumed by the Gateway       |
| Tools / integration catalog   | Composio OAuth flow + connection metadata                               |
| Realtime SSE                  | Broadcast channel for table-change events                               |
| Structured logs               | Hot tier (Postgres, 7 days) + cold archive (R2 gzip NDJSON)            |
| Distributed tracing           | `x-request-id` propagated across all services                          |

---

## 2. Control Plane vs Execution Plane

> **This is a core architecture invariant. It must never be violated.**

Fluxbase uses a strict two-plane model. Mixing these planes leads to scaling
problems, security boundary confusion, and operational complexity.

### Control Plane — `api.fluxbase.co`

| Attribute     | Value                                                                  |
|---------------|------------------------------------------------------------------------|
| Domain        | `api.fluxbase.co`                                                      |
| Service       | `fluxbase-api`                                                         |
| Traffic volume| **Low** — management operations only                                   |
| Callers       | CLI, dashboard, SDK setup, CI/CD pipelines                             |

**Handles:**
- Tenant, project, member management
- Function registration and deployment
- Secret management
- API key management
- Gateway route configuration
- Schema graph and SDK generation
- Logs and distributed traces
- Integration (OAuth) setup

**Does NOT handle:**
- Function invocation
- Webhook delivery
- Agent or workflow execution
- Database queries from user code
- LLM tool execution
- Any high-frequency runtime traffic

### Execution Plane — `{tenant_slug}.fluxbase.co`

| Attribute     | Value                                                                  |
|---------------|------------------------------------------------------------------------|
| Domain        | `{tenant_slug}.fluxbase.co` (e.g. `acme.fluxbase.co`)                 |
| Services      | `fluxbase-gateway` → `fluxbase-runtime`                               |
| Traffic volume| **Very high** — 1,000s–100,000s rps                                   |
| Callers       | End users, webhooks, external systems, SDKs at runtime                 |

**Handles:**
- Function invocation (`POST acme.fluxbase.co/create_user`)
- Webhook endpoints (`POST acme.fluxbase.co/webhook/stripe`)
- Public HTTP APIs
- Agent and workflow execution
- Database queries via the Data Engine
- LLM tool execution via Composio

### Platform Comparison

| Platform    | Control Plane      | Execution Plane       |
|-------------|--------------------|-----------------------|
| Cloudflare  | `api.cloudflare.com` | Workers edge          |
| Vercel      | `vercel.com/api`   | Edge runtime          |
| Supabase    | `api.supabase.com` | `*.supabase.co`       |
| Stripe      | Dashboard API      | Payment network       |
| **Fluxbase**| `api.fluxbase.co`  | `{slug}.fluxbase.co`  |

### Architectural Rule — Enforcement at the Code Level

The API service explicitly blocks all execution-path requests. Routes that
look like function invocation (`/run`, `/invoke`, `/execute`) return:

```json
HTTP 405 Method Not Allowed
{
  "error": "execution_not_allowed_on_control_plane",
  "message": "Function execution must go through the Gateway. Use https://{tenant_slug}.fluxbase.co/{function_name}"
}
```

This is not a documentation guideline — it is enforced in `main.rs` so
architectural drift cannot slip through unnoticed.

### CLI Mental Model

```
flux deploy         →  api.fluxbase.co          (control plane)
flux logs           →  api.fluxbase.co          (control plane)
flux trace <id>     →  api.fluxbase.co          (control plane)

curl acme.fluxbase.co/my_fn  →  gateway → runtime  (execution plane)
```

---

## 3. Module Structure

```
api/src/
├── main.rs                   Router assembly, AppState, startup
├── config/
│   └── mod.rs                Env loading, tracing initialisation
├── db/
│   ├── connection.rs         PgPool construction
│   └── queries.rs            Named SQL helpers
├── middleware/
│   ├── auth.rs               Firebase JWT + API key verification
│   ├── context.rs            Tenant/project membership resolution
│   ├── scope.rs              Platform / Tenant / Project guard
│   ├── request_id.rs         x-request-id propagation + request logging
│   ├── api_key_auth.rs       API key extraction helpers
│   └── mod.rs
├── models/
│   ├── user.rs               User DB row
│   ├── tenant.rs             Tenant DB row
│   ├── project.rs            Project DB row
│   └── membership.rs         TenantMember row
├── types/
│   ├── context.rs            RequestContext (user_id, tenant_id, project_id, role, slugs)
│   ├── response.rs           ApiResponse<T>, ApiError (uniform JSON error format)
│   ├── scope.rs              Scope enum { Platform, Tenant, Project }
│   └── mod.rs
├── services/
│   ├── auth_service.rs       User upsert logic
│   ├── tenant_service.rs     Tenant create + owner membership
│   ├── project_service.rs    Project create
│   ├── slug_service.rs       Slug generation (name → kebab-case + uniqueness)
│   └── storage.rs            S3/R2 client (put, get, presign, delete)
├── secrets/
│   ├── encryption.rs         AES-256-GCM encrypt / decrypt
│   ├── service.rs            Secret CRUD (versioned)
│   ├── model.rs              Secret / SecretVersion DB rows
│   ├── dto.rs                CreateSecretRequest, UpdateSecretRequest
│   ├── events.rs             Secret-related domain events
│   └── routes.rs             HTTP handlers
├── api_keys/
│   ├── crypto.rs             Key generation + SHA-256 hashing
│   ├── service.rs            DB CRUD + mark_key_used
│   ├── model.rs              ApiKey DB row + request/response DTOs
│   └── routes.rs             HTTP handlers
├── logs/
│   ├── archiver.rs           Background archival to S3/R2
│   ├── routes.rs             Ingest, list, trace endpoints
│   └── mod.rs
└── routes/
    ├── auth.rs               GET /auth/me, POST /auth/logout
    ├── tenants.rs            Tenant CRUD + member management
    ├── projects.rs           Project CRUD
    ├── functions.rs          Function registry CRUD
    ├── deployments.rs        Deployment CRUD + CLI multipart upload
    ├── secrets.rs            (delegates to secrets/ module)
    ├── gateway_routes.rs     Gateway route config CRUD
    ├── schema.rs             Schema graph proxy
    ├── sdk.rs                TypeScript SDK generation
    ├── openapi.rs            OpenAPI 3.0 spec generation
    ├── events.rs             SSE stream + internal emit
    ├── tools.rs              Tool catalog, Composio OAuth
    ├── data_engine.rs        Generic /db/* and /files/* proxy
    ├── platform.rs           Platform runtimes + services listing
    ├── demo.rs               Public demo endpoints (signup trace)
    ├── system.rs             Health / version
    └── mod.rs
```

---

## 4. AppState

`AppState` is cloned into every handler via Axum's `State` extractor.

```rust
pub struct AppState {
    pub pool:            sqlx::PgPool,
    pub firebase_auth:   Arc<FirebaseAuth>,
    pub storage:         StorageService,
    pub storage_config:  StorageConfig,        // bucket names per tier
    pub http_client:     reqwest::Client,
    pub data_engine_url: String,               // DATA_ENGINE_URL env var
    pub gateway_url:     String,               // GATEWAY_URL env var (used in OpenAPI servers[])

    /// In-memory TypeScript SDK cache.
    /// Key: "{project_id}:{schema_hash}"  — auto-invalidated on schema change.
    pub sdk_cache:    Arc<tokio::sync::RwLock<HashMap<String, String>>>,

    /// Broadcast channel for SSE (table-change events).
    /// Capacity: 1024. Lagging receivers drop messages (non-blocking).
    pub event_tx:     tokio::sync::broadcast::Sender<String>,

    /// Background log archiver — moves hot Postgres logs → cold R2/S3.
    pub log_archiver: Arc<LogArchiver>,
}
```

---

## 5. Middleware Stack

Middleware is applied from **outermost (first applied) to innermost (closest to handler)**:

```
Request
  │
  ▼  request_id_middleware       ← assign/propagate x-request-id, log every request
  │
  ▼  CORS (tower-http CorsLayer) ← handle OPTIONS preflight before auth
  │
  ▼  DefaultBodyLimit (1 MB)
  │
  ▼  verify_auth                 ← Firebase JWT or flux_* API key
  │
  ▼  resolve_context             ← tenant/project membership, slugs
  │
  ▼  require_scope               ← Platform / Tenant / Project guard
  │
  ▼  Handler
```

### `request_id_middleware`
- Reads `x-request-id` from incoming request headers.
- If absent, generates a new UUID v4.
- Writes it back onto the response headers.
- Logs: `{method} {path} → {status} ({latency}ms) [{request_id}]` for all non-`/health` routes.

### `verify_auth`
- Skips OPTIONS (CORS preflight).
- Detects token type by prefix: `flux_` → API key path; otherwise Firebase JWT path.
- On success, inserts `RequestContext` into request extensions.

### `resolve_context`
- Reads `X-Fluxbase-Tenant` header → validates UUID → checks `tenant_members`
  → populates `context.tenant_id`, `context.role`, `context.tenant_slug`.
- If tenant valid and `X-Fluxbase-Project` header present → validates UUID → checks
  `projects WHERE tenant_id = $tenant` → populates `context.project_id`,
  `context.project_slug`.
- API key requests skip this step (already populated by `verify_auth`).

### `require_scope`
- `Platform`: authentication sufficient.
- `Tenant`: `context.tenant_id` must be `Some`.
- `Project`: both `context.tenant_id` and `context.project_id` must be `Some`.

---

## 6. Scope & Authorization Model

```
User
 └── Tenant (created by user → becomes owner)
      ├── TenantMembers (owner | member | viewer)
      └── Project
           ├── Functions
           ├── Deployments
           ├── Secrets
           ├── API Keys
           └── Gateway Routes
```

Three scopes, enforced at the middleware layer:

| Scope    | Required headers                                     | DB checks                                               |
|----------|------------------------------------------------------|---------------------------------------------------------|
| Platform | `Authorization: Bearer <token>`                      | Firebase JWT valid / API key valid                      |
| Tenant   | + `X-Fluxbase-Tenant: <uuid>`                        | Row in `tenant_members` for (tenant_id, user_id)        |
| Project  | + `X-Fluxbase-Project: <uuid>`                       | Row in `projects` WHERE tenant_id matches               |

Role is preserved in `context.role` (`owner` / `member` / `viewer`). Individual
handlers are responsible for enforcing role-level restrictions beyond scope.

---

## 7. Route Groups

### 7.1 Platform Routes

Require `Authorization` only (Platform scope).

| Method | Path                     | Handler                       | Description                            |
|--------|--------------------------|-------------------------------|----------------------------------------|
| GET    | `/auth/me`               | `auth::get_me`                | Return current user info               |
| POST   | `/auth/logout`           | `auth::logout`                | Revoke session (client-side hint)      |
| GET    | `/platform/runtimes`     | `platform::list_runtimes`     | List active runtime environments       |
| GET    | `/platform/services`     | `platform::list_services`     | List registered platform services      |
| POST   | `/tenants`               | `tenants::create_tenant`      | Create tenant, adds creator as owner   |
| GET    | `/tenants`               | `tenants::get_tenants`        | List tenants the current user belongs to |
| GET    | `/tenants/{id}`          | `tenants::get_tenant`         | Get tenant by ID                       |
| DELETE | `/tenants/{id}`          | `tenants::delete_tenant`      | Delete tenant (owner only)             |

### 7.2 Tenant Routes

Require `Authorization` + `X-Fluxbase-Tenant` (Tenant scope).

| Method | Path                            | Handler                       | Description                          |
|--------|---------------------------------|-------------------------------|--------------------------------------|
| GET    | `/tenants/{id}/members`         | `tenants::get_members`        | List tenant members with roles       |
| POST   | `/tenants/{id}/members`         | `tenants::invite_member`      | Invite user to tenant                |
| DELETE | `/tenants/{id}/members/{user}`  | `tenants::remove_member`      | Remove member from tenant            |
| GET    | `/projects`                     | `projects::get_projects`      | List projects in tenant              |
| POST   | `/projects`                     | `projects::create_project`    | Create project under tenant          |
| GET    | `/projects/{id}`                | `projects::get_project`       | Get project by ID                    |
| DELETE | `/projects/{id}`                | `projects::delete_project`    | Delete project                       |
| DELETE | `/api-keys/{id}`                | `api_keys::revoke_api_key`    | Revoke API key (also Tenant-scoped)  |

### 7.3 Project Routes

Require `Authorization` + `X-Fluxbase-Tenant` + `X-Fluxbase-Project` (Project scope).

#### Functions & Deployments

| Method | Path                                         | Description                                  |
|--------|----------------------------------------------|----------------------------------------------|
| GET    | `/functions`                                 | List functions with run URLs                 |
| POST   | `/functions`                                 | Create function (validates runtime registry) |
| GET    | `/functions/{id}`                            | Get function by ID                           |
| DELETE | `/functions/{id}`                            | Delete function                              |
| POST   | `/functions/deploy`                          | CLI multipart deploy (upsert + new version)  |
| POST   | `/deployments`                               | Create deployment from existing storage_key  |
| GET    | `/deployments/list/{function_name}`          | List deployments for a function              |
| POST   | `/deployments/{name}/activate/{version}`     | Set a specific version active                |

#### Secrets

| Method | Path               | Description                                  |
|--------|--------------------|----------------------------------------------|
| GET    | `/secrets`         | List secrets (names + metadata, no values)   |
| POST   | `/secrets`         | Create secret (AES-256-GCM encrypted)        |
| PUT    | `/secrets/{key}`   | Update secret (creates new version)          |
| DELETE | `/secrets/{key}`   | Delete secret                                |

#### API Keys

| Method | Path          | Description                                  |
|--------|---------------|----------------------------------------------|
| GET    | `/api-keys`   | List active API keys (no hash exposed)       |
| POST   | `/api-keys`   | Create key — plaintext returned **once only**|

#### Gateway Routes

| Method | Path            | Description                                |
|--------|-----------------|--------------------------------------------|
| GET    | `/routes`       | List gateway route configs                 |
| POST   | `/routes`       | Create route config                        |
| PATCH  | `/routes/{id}`  | Update route config                        |
| DELETE | `/routes/{id}`  | Delete route config                        |

#### Schema, SDK & OpenAPI

| Method | Path              | Description                                               |
|--------|-------------------|-----------------------------------------------------------|
| GET    | `/schema/graph`   | Unified table + function metadata graph (proxied from DE) |
| GET    | `/sdk/schema`     | Raw schema JSON for SDK consumers                        |
| GET    | `/sdk/typescript` | Auto-generated TypeScript SDK (cached per schema hash)   |
| GET    | `/openapi.json`   | OpenAPI 3.0 spec generated from live schema              |

#### Events, Logs & Traces

| Method | Path                        | Description                                          |
|--------|-----------------------------|------------------------------------------------------|
| GET    | `/events/stream`            | SSE stream — subscribe to table-change events        |
| GET    | `/logs`                     | Query function logs (hot + archive)                  |
| GET    | `/traces/{request_id}`      | Reconstruct full distributed trace across services   |

#### Tools & Integrations

| Method | Path                              | Description                                    |
|--------|-----------------------------------|------------------------------------------------|
| GET    | `/tools`                          | Full tool catalog annotated with connect status|
| GET    | `/tools/connected`                | Active integrations only                       |
| POST   | `/tools/connect/{provider}`       | Start OAuth flow — returns redirect URL        |
| DELETE | `/tools/disconnect/{provider}`    | Remove integration + revoke Composio token     |

#### Data Engine Proxy

| Method | Path          | Description                                                   |
|--------|---------------|---------------------------------------------------------------|
| ANY    | `/db/{*path}` | Proxy all DB management calls to Data Engine                  |
| ANY    | `/files/{*path}` | Proxy file management calls to Data Engine                 |

### 7.4 Internal Routes

Mounted at `/internal/`. Not authenticated with Firebase/API key — protected by
`X-Service-Token` header (checked per-handler).

| Method | Path               | Caller    | Description                                        |
|--------|--------------------|-----------|----------------------------------------------------|
| GET    | `/secrets`         | Runtime   | Decrypted secrets map for `(tenant_id, project_id)`|
| GET    | `/bundle`          | Runtime   | Active deployment bundle (presigned URL or inline) |
| POST   | `/logs`            | All       | Ingest structured log span from any service        |
| GET    | `/logs`            | Internal  | Query logs by tenant/project                       |
| POST   | `/events/emit`     | Runtime   | Broadcast table-change event to SSE subscribers    |

### 7.5 Public Routes

No authentication required.

| Method | Path                          | Description                                              |
|--------|-------------------------------|----------------------------------------------------------|
| POST   | `/demo/signup`                | Rate-limited demo signup with trace generation           |
| GET    | `/demo/trace/{request_id}`    | Fetch trace for demo signup flow                         |
| GET    | `/tools/oauth/callback`       | Composio OAuth redirect target                           |
| GET    | `/health`                     | `{ "status": "ok" }` — for Cloud Run health checks       |
| GET    | `/version`                    | `{ "service", "commit", "build_time" }`                  |

---

## 8. Authentication

### 8.1 Firebase JWT

Standard Firebase Auth flow. The API verifies the JWT against Google's JWK set
using the `firebase-auth` crate. On success:

1. Extracts `firebase_uid` and `email` from the token claims.
2. Upserts a row in `users (firebase_uid, email)` — returns the internal `user_id` UUID.
3. Inserts `RequestContext { user_id, firebase_uid, ... }` into request extensions.

In test mode (`#[cfg(test)]`), the JWT verification is bypassed and a
`mock-uid-<token>` is used.

### 8.2 API Keys (`flux_*`)

Format: `flux_<base64url(32 random bytes)>` — 48 characters after the prefix.

| Step | Detail                                                                          |
|------|---------------------------------------------------------------------------------|
| Generate | `rand::thread_rng().fill_bytes(32)`, base64url-encode, prepend `flux_`      |
| Store | SHA-256 hash of the full plaintext stored in `api_keys.key_hash`               |
| Plaintext | Returned **once** at creation, never stored                                 |
| Verify | Hash incoming token, query `api_keys WHERE key_hash = $1 AND revoked = false` |
| Usage | `mark_key_used` sets `last_used_at = now()`                                    |
| Scope | Keys are scoped to a specific tenant + project (pre-populated into context)     |
| Revoke | Sets `revoked = true` (soft-delete, remains queryable for audit)               |

API key auth also resolves the tenant owner's user UUID so write operations
(which have a FK to `users.id`) work correctly.

### 8.3 Internal Service Token

`/internal/*` routes that need extra protection check:

```
X-Service-Token: <INTERNAL_SERVICE_TOKEN env var>
```

Returns `401 { error: "invalid_service_token" }` on mismatch.

---

## 9. Functions

A **Function** is the unit of deployable logic on Fluxbase.

### Schema

```
functions
  id            UUID PK
  tenant_id     UUID FK tenants
  project_id    UUID FK projects
  name          TEXT (unique per project)
  runtime       TEXT (validated against platform_runtimes.name WHERE status='active')
  description   TEXT nullable
  input_schema  JSONB nullable
  output_schema JSONB nullable
  created_at    TIMESTAMPTZ
```

### Run URL

Each function gets a deterministic run URL based on tenant slug:

```
https://{tenant_slug}.fluxbase.co/{function_name}
```

### Create function

- Validates `runtime` against `platform_runtimes` (rejects inactive runtimes).
- Inserts with empty schema — schema is populated on first CLI deploy.

---

## 10. Deployments

A **Deployment** is a versioned, immutable snapshot of a function's code bundle.

### Schema

```
deployments
  id           UUID PK
  function_id  UUID FK functions
  storage_key  TEXT           — legacy key in functions bucket
  bundle_code  TEXT nullable  — inline fallback copy of the bundle
  bundle_url   TEXT nullable  — S3/R2 object key (preferred)
  version      INT            — monotonically increasing per function
  status       TEXT           — 'ready' | 'error'
  is_active    BOOL           — only one active deployment per function
  created_at   TIMESTAMPTZ
```

### CLI Deploy (`POST /functions/deploy`)

Accepts `multipart/form-data` with fields:

| Field          | Required | Description                                    |
|----------------|----------|------------------------------------------------|
| `name`         | ✓        | Function name                                  |
| `runtime`      | ✓        | Runtime identifier                             |
| `bundle`       | ✓        | JavaScript bundle bytes                        |
| `description`  |          | Human-readable description                     |
| `input_schema` |          | JSON Schema string for input validation        |
| `output_schema`|          | JSON Schema string for output shape            |

Process:
1. Look up function by (name, project_id) — create if not found.
2. Update `input_schema` / `output_schema` / `description` if provided.
3. Upload bundle to R2 at `bundles/{tenant_id}/{project_id}/{deployment_id}.js`.
4. Within a transaction: deactivate all deployments for function, insert new deployment as active.
5. Return `{ function_id, deployment_id, version, url }`.

### Bundle Fetch (`GET /internal/bundle`)

Used by the runtime engine to warm-load a function before execution.
Query param: `function_id` (UUID or function name).

Returns:
- `{ deployment_id, url }` — presigned S3 URL (5-minute TTL) if `bundle_url` is set.
- `{ deployment_id, code }` — inline bundle code as fallback.

---

## 11. Secrets

Secrets are stored encrypted in Postgres and decrypted on demand at runtime.

### Encryption

- Algorithm: **AES-256-GCM** (authenticated encryption)
- Key Material: `SECRET_ENCRYPTION_KEY` env var (32-byte base64 or raw hex key)
- Nonce: random 12-byte per encryption, prepended to ciphertext
- Output: `base64(nonce || ciphertext || tag)` stored in `secret_versions.encrypted_value`

### Schema

```
secrets
  id          UUID
  tenant_id   UUID FK tenants
  project_id  UUID nullable FK projects  — null = tenant-wide
  key         TEXT (unique per tenant+project)
  created_at  TIMESTAMPTZ

secret_versions
  id               UUID
  secret_id        UUID FK secrets
  version          INT  (monotonically increasing)
  encrypted_value  TEXT
  created_at       TIMESTAMPTZ
```

### Internal Secrets Endpoint

`GET /internal/secrets?tenant_id=&project_id=`

Returns a flat map:

```json
{
  "DATABASE_URL": "postgres://...",
  "STRIPE_KEY": "sk_live_...",
  ...
}
```

This is fetched by the runtime before invoking a function handler.

---

## 12. API Keys Management

See [Authentication § 8.2](#82-api-keys-flux_) for the cryptographic details.

### Endpoints summary

| Operation | Endpoint                  | Scope   | Notes                                       |
|-----------|---------------------------|---------|---------------------------------------------|
| Create    | `POST /api-keys`          | Project | Returns plaintext key **once**              |
| List      | `GET /api-keys`           | Project | Returns metadata — `key_hash` not exposed   |
| Revoke    | `DELETE /api-keys/{id}`   | Tenant  | Soft-delete (`revoked = true`)              |

---

## 13. Schema Graph & SDK

### Schema Graph (`GET /schema/graph`)

Proxied to the Data Engine service. The API:
1. Forwards `X-Fluxbase-Tenant-Slug` and `X-Fluxbase-Project-Slug` headers.
2. Also forwards `x-request-id` for distributed tracing.
3. Guards the response: checks HTTP status before attempting JSON parse; surfaces
   upstream errors as `data_engine_error(<status>): <body>`.

The schema graph contains:
- Tables with columns, types, constraints
- Relationships between tables
- Deployed functions with input/output schemas
- Row-level security policies

### TypeScript SDK (`GET /sdk/typescript`)

Generates a typed TypeScript client from the live schema graph.

- Cached in `AppState.sdk_cache` keyed by `"{project_id}:{schema_hash}"`.
- Cache is auto-invalidated when the schema changes (new hash ≠ cached key).
- Generated code includes typed query functions, table interfaces, and type-safe event subscriptions.

### OpenAPI (`GET /openapi.json`)

Generates an OpenAPI 3.0 specification from the live schema + function list.
`servers[0].url` is set to `AppState.gateway_url` so the spec points to the
execution endpoints on the Gateway.

---

## 14. Gateway Routes

Gateway routes define how incoming HTTP requests at the Gateway are dispatched
to function handlers.

### Schema

```
gateway_routes
  id               UUID PK
  project_id       UUID FK projects
  path_pattern     TEXT   e.g. "/users/:id"
  function_name    TEXT
  method           TEXT   e.g. "POST" (or "*" for any)
  is_async         BOOL   — if true, gateway returns 202 and enqueues
  middleware_config JSONB  — rate limiting, auth, transform config
  created_at       TIMESTAMPTZ
```

### CRUD

| Operation | Notes                                                                       |
|-----------|-----------------------------------------------------------------------------|
| Create    | POST `/routes` — validates uniqueness of (project, path_pattern, method)    |
| Update    | PATCH `/routes/{id}` — partial update via JSON merge                        |
| Delete    | DELETE `/routes/{id}` — removes route; gateway picks up change at next poll |

The Gateway service polls or watches this table to build its routing table.

---

## 15. Tools & Integrations

### Tool Catalog

A static list of ~20 tools across 9 providers is embedded in `routes/tools.rs`:

| Provider       | Tools included                                                  |
|----------------|-----------------------------------------------------------------|
| Slack          | send_message, create_channel, get_messages                      |
| GitHub         | create_issue, close_issue, comment_issue, create_pr, merge_pr   |
| Gmail          | send_email, get_emails                                          |
| Linear         | create_issue, update_issue                                      |
| Notion         | create_page, search                                             |
| Jira           | create_issue, update_issue, comment_issue                       |
| Airtable       | create_record, list_records                                     |
| Google Sheets  | append_row                                                      |
| Stripe         | create_customer, create_charge                                  |

`GET /tools` annotates each tool with `"connected": true/false` based on the
project's active `integrations` rows.

### OAuth Flow

```
Client                   API                      Composio
  │                       │                           │
  │ POST /tools/connect   │                           │
  │ /slack                │                           │
  │──────────────────────►│                           │
  │                       │ POST /api/v2/connections  │
  │                       │──────────────────────────►│
  │                       │ ← { redirectUrl }         │
  │ ← { redirect_url }    │                           │
  │                       │                           │
  │ (user authorizes at Composio/provider)            │
  │                       │                           │
  │ GET /tools/oauth/     │                           │
  │ callback?connected_   │◄──────────────────────────│
  │ account_id=...        │                           │
  │──────────────────────►│                           │
  │                       │ UPDATE integrations       │
  │                       │ SET status = 'active'     │
  │ ← 302 to dashboard    │                           │
```

Connection metadata is stored in:

```
integrations
  id                     UUID
  project_id             UUID FK projects
  provider               TEXT  e.g. "slack"
  account_label          TEXT nullable
  composio_connection_id TEXT nullable
  status                 TEXT  'pending' | 'active' | 'error'
  metadata               JSONB
  connected_at           TIMESTAMPTZ nullable
  created_at             TIMESTAMPTZ
```

The `entity_id` in Composio is the tenant UUID (so all projects under a tenant
share OAuth credentials). `COMPOSIO_ENTITY_ID` env var can override this for
shared demo accounts.

---

## 16. SSE Events

Real-time table-change events are distributed using an in-process broadcast channel
(capacity: 1024 messages).

### Architecture

```
Runtime/Data Engine
        │
        ▼
POST /internal/events/emit
        │
        ▼
 AppState.event_tx.send(json_message)
        │
        ▼ (broadcast)
Multiple SSE handlers         ← one tokio task per connected client
        │
        ▼
Client EventSource (dashboard)
```

### Message format

```json
{
  "project_id": "3787e1fa-...",
  "table":      "users",
  "operation":  "insert",
  "row":        { ... }
}
```

### Client subscription

`GET /events/stream` — returns `text/event-stream`.

Clients filter by their own `project_id`. Lagging receivers (slow clients) simply
miss messages — the broadcast is non-blocking and will not block the publisher.

---

## 17. Logs & Observability

### Two-tier storage

| Tier  | Store          | Retention         | Format              |
|-------|----------------|-------------------|---------------------|
| Hot   | Postgres       | `LOG_HOT_DAYS` (default 7) | `platform_logs` rows |
| Cold  | R2 / S3        | Indefinite        | gzip-compressed NDJSON |

### `platform_logs` schema

```
platform_logs
  id          UUID PK
  tenant_id   UUID
  project_id  UUID nullable
  source      TEXT   — service name: 'api' | 'gateway' | 'runtime' | ...
  resource_id TEXT   — function name, route name, etc.
  level       TEXT   — 'trace' | 'debug' | 'info' | 'warn' | 'error'
  message     TEXT
  request_id  TEXT nullable  — correlates spans across services
  metadata    JSONB nullable
  timestamp   TIMESTAMPTZ
```

### Log Archiver

`logs/archiver.rs` runs as a background Tokio task:

- Wakes after 5 minutes on startup, then every hour.
- Fetches up to `LOG_ARCHIVE_BATCH` (default 5000) rows older than `LOG_HOT_DAYS`.
- Groups rows by `(tenant_id, source, resource_id, date, hour)`.
- For each group: serialises as NDJSON, gzip-compresses, uploads to S3/R2.
- Object key pattern: `logs/{tenant_id}/{YYYY}/{MM}/{DD}/{source}/{resource_id}/{HH}-{epoch_ms}.ndjson.gz`
- Deletes archived rows from Postgres only after successful upload.
- Upload failures are silently retried next cycle.

### Log Query (`GET /logs`)

Query parameters:

| Param      | Type             | Description                                                   |
|------------|------------------|---------------------------------------------------------------|
| `function` | string           | Filter by function name or resource_id                        |
| `level`    | string           | Filter by log level                                           |
| `limit`    | integer          | Max rows returned (default 100)                               |
| `since`    | RFC3339 datetime | Lower bound on timestamp — triggers archive fetch if outside hot window |

When `since` is outside the hot window, the archiver's `fetch_archived` method
is called to page through cold-tier S3 objects transparently.

### Distributed Traces (`GET /traces/{request_id}`)

Returns all log spans with the given `request_id` across all services and sources,
ordered by timestamp. Used to reconstruct the full execution path of a single
request across API → Gateway → Runtime → Data Engine.

### `x-request-id` propagation

Every outbound `reqwest` call made by the API (to Data Engine, etc.) includes
`x-request-id` in the request headers, enabling end-to-end correlation.

---

## 18. Data Engine Proxy

All routes matched by `/db/{*path}` and `/files/{*path}` are **transparently
proxied** to `DATA_ENGINE_URL`.

### Headers forwarded

| Header                        | Source                              |
|-------------------------------|-------------------------------------|
| `Authorization`               | Forwarded as-is from client         |
| `X-Fluxbase-Tenant`           | Forwarded as-is                     |
| `X-Fluxbase-Project`          | Forwarded as-is                     |
| `X-Fluxbase-Tenant-Slug`      | Injected from `context.tenant_slug` |
| `X-Fluxbase-Project-Slug`     | Injected from `context.project_slug`|
| `x-request-id`                | Propagated for distributed tracing  |
| `Content-Type`                | Forwarded                           |

### Response Guard

Before attempting JSON deserialization, the proxy checks that the upstream
returned a 2xx status. Non-2xx responses are surfaced as:

```json
{ "error": "data_engine_error(502)", "message": "<upstream body>" }
```

---

## 19. Storage

The `StorageService` wraps an AWS S3-compatible client (Cloudflare R2 in
production, MinIO locally).

### Bucket layout

| Bucket (env var)             | Contents                                         |
|------------------------------|--------------------------------------------------|
| `FUNCTIONS_BUCKET`           | Function bundles: `bundles/{tenant_id}/{project_id}/{deployment_id}.js` |
| `FILES_BUCKET`               | User-uploaded files managed via Data Engine      |
| `LOG_BUCKET`                 | Archived log files: `logs/.../*.ndjson.gz`       |

### Operations

| Method                                            | Use                                |
|---------------------------------------------------|------------------------------------|
| `put_object(key, bytes, content_type)`            | Upload bundle / log                |
| `get_object(key)`                                 | Download bundle                    |
| `presigned_get_object(key, duration)`             | Generate time-limited download URL |
| `delete_object(key)`                              | Remove file                        |

---

## 20. CORS & Security

### CORS

Configured via `ALLOWED_ORIGINS` env var (comma-separated list):

```
ALLOWED_ORIGINS=http://localhost:5173,https://app.fluxbase.co,https://fluxbase.co
```

Additional implicit allowlist:
- Any origin matching `*.fluxbase.co`
- `https://fluxbase.co`

Allowed methods: `GET POST PUT PATCH DELETE OPTIONS`  
Allowed headers: `Authorization`, `Content-Type`, `Accept`, `X-Fluxbase-Tenant`,
`X-Fluxbase-Project`  
`allow_credentials: true` — required for dashboard cookie-based sessions.  
CORS cache (`max_age`): 1 hour.

### Body Limit

1 MB maximum request body (`DefaultBodyLimit::max(1 * 1024 * 1024)`).

### 404 Fallback

Unmatched routes return:

```json
{ "error": "not_found", "path": "/unmatched/route" }
```

A warning is logged at `WARN` level.

---

## 21. Environment Variables

| Variable                          | Default                        | Required | Description                                   |
|-----------------------------------|--------------------------------|----------|-----------------------------------------------|
| `DATABASE_URL`                    | —                              | ✓        | PostgreSQL connection string                  |
| `FIREBASE_PROJECT_ID`             | —                              | ✓        | Firebase project for JWT verification         |
| `DATA_ENGINE_URL`                 | `http://localhost:8082`        |          | Internal URL to Data Engine service           |
| `GATEWAY_URL`                     | `http://localhost:8081`        |          | Gateway URL (surfaced in OpenAPI spec)        |
| `PORT`                            | `8080`                         |          | Listening port                                |
| `ALLOWED_ORIGINS`                 | `http://localhost:5173`        |          | Comma-separated CORS origins                  |
| `INTERNAL_SERVICE_TOKEN`          | `stub_token`                   |          | Token for `/internal/*` endpoints             |
| `SECRET_ENCRYPTION_KEY`           | —                              | ✓ (prod) | AES-256-GCM key for secret encryption        |
| `R2_ENDPOINT` / `S3_ENDPOINT`     | `http://127.0.0.1:9000`        |          | Object store endpoint                         |
| `R2_ACCESS_KEY_ID` / `S3_ACCESS_KEY_ID` | `minioadmin`            |          | Object store credentials                      |
| `R2_SECRET_ACCESS_KEY` / `S3_SECRET_ACCESS_KEY` | `minioadmin`    |          | Object store secret                           |
| `LOG_BUCKET`                      | `fluxbase-logs`                |          | Bucket for archived log files                 |
| `LOG_HOT_DAYS`                    | `7`                            |          | Days to keep logs in Postgres                 |
| `LOG_ARCHIVE_BATCH`               | `5000`                         |          | Max log rows per archival cycle               |
| `COMPOSIO_API_KEY`                | —                              |          | Composio key for tool connectivity            |
| `COMPOSIO_ENTITY_ID`              | tenant UUID                    |          | Override Composio entity (demo / shared)      |
| `GIT_SHA`                         | `unknown`                      |          | Injected at build time for `/version`         |
| `BUILD_TIME`                       | `unknown`                      |          | Injected at build time for `/version`         |

---

## 22. Known Issues & Improvement Areas

The following items were identified during code review. Prioritised for discussion:

### High priority

| # | Area | Issue | Suggested fix |
|---|------|-------|---------------|
| 1 | **Event bus** | `POST /functions` and `POST /deployments` have `// TODO: Publish to actual event bus` + `println!` stubs | Wire to `AppState.event_tx` or an external queue (NATS / PubSub) |
| 2 | **Role enforcement** | Tenant `role` is stored in context but **no handler checks it** — all members can do owner actions | Add role guards to destructive operations (delete tenant, remove member) |
| 3 | **Internal route auth** | `/internal/logs` (GET) does not check `X-Service-Token` — only the POST and secrets endpoints do | Add token check consistently to all `/internal/*` routes |
| 4 | **Secret key** | `SECRET_ENCRYPTION_KEY` falls back to a dev stub in several paths — encryption silently uses weak key in dev | Fail-fast in non-`test` builds if key is missing or < 32 bytes |
| 5 | **Multipart bundle size** | CLI deploy accepts `bundle` bytes without size validation — only the global 1 MB body limit applies | Add per-field size check; consider raising the global limit for deploy only |

### Medium priority

| # | Area | Issue | Suggested fix |
|---|------|-------|---------------|
| 6 | **SDK cache invalidation** | Cache keyed on `project_id:schema_hash` — but schema hash is not checked on every request; stale cache possible if schema changes between requests | Recompute hash on each `/sdk/typescript` request and use as cache key |
| 7 | **Gateway routes poll** | Gateway consumes the `gateway_routes` table via polling — no notification mechanism | Add a Postgres `LISTEN`/`NOTIFY` push channel or a `/routes/changes` SSE endpoint |
| 8 | **Test coverage** | Integration tests in `main.rs` require a live DB and are not isolated — they share the database state between test runs | Use transaction-scoped tests or a dedicated test DB with migrations |
| 9 | **Composio entity scope** | `entity_id` defaults to `tenant_id` but Composio tokens are project-scoped in the `integrations` table — tenant-wide vs project-wide is ambiguous | Decide and document the intended OAuth scope granularity |
| 10 | **OpenAPI spec** | The spec is generated from the Data Engine schema — function route parameters and auth requirements are not represented | Augment the spec with function handler metadata from `functions.input_schema` / `output_schema` |

### Low priority / Nice-to-have

| # | Area | Issue |
|---|------|-------|
| 11 | **Secrets versioning** | Old secret versions are never pruned — table grows unbounded over time |
| 12 | **`revoked` API keys** | Revoked keys remain queryable; no scheduled cleanup or TTL |
| 13 | **CORS origin match** | `origin_lc.ends_with(".fluxbase.co")` would match `evilfluxbase.co` — use exact subdomain check |
| 14 | **Deployment delete** | No `DELETE /deployments/{id}` endpoint — removing a function leaves orphaned R2 objects |
| 15 | **Structured event schema** | SSE messages are freeform JSON strings — no schema validation or versioning |
