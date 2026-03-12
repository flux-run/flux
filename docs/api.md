# API Service

> **Internal architecture doc.** This describes the API service implementation
> for contributors. For user-facing docs, see [framework.md](framework.md).

---

## Overview

| Property | Value |
|---|---|
| Service name | `flux-api` |
| Role | Control plane — management operations |
| Tech | Rust, Axum, SQLx, PostgreSQL |
| Default port | `:8080` |
| Exposed to internet | No (behind gateway in production) |

The API service is the control plane. Every management action (create function,
deploy code, manage secrets, configure routes, browse logs) passes through it.
It is **not** the execution hot path — that belongs to Gateway + Runtime.

```
CLI / Dashboard
      │
      ▼
 API :8080  ──► PostgreSQL
      │
      ├── Function registry (metadata + schema)
      ├── Deployment management (bundles in R2/S3)
      ├── Secret management (AES-256-GCM encrypted)
      ├── Schema graph (proxied from Data Engine)
      ├── Log access (hot tier: Postgres, cold: R2 archive)
      └── Gateway route config (consumed by Gateway snapshot)
```

---

## Control plane vs execution plane

This is a core invariant. The API service handles low-volume management traffic.
The Gateway + Runtime handle high-volume execution traffic. Never mix them.

| Plane | Service | Traffic | Callers |
|---|---|---|---|
| Control | API `:8080` | Low — management ops | CLI, dashboard, CI/CD |
| Execution | Gateway `:4000` → Runtime `:8083` | High — user requests | End users, webhooks |

---

## Route groups

### Platform routes (super admin)
- `POST /platform/tenants` — create tenant
- `GET /platform/stats` — platform-wide stats

### Tenant routes
- `GET /tenants/:id` — tenant details
- `PUT /tenants/:id` — update tenant

### Project routes (most CLI commands hit these)
- `POST /projects/:id/functions` — create function
- `POST /projects/:id/deploy` — deploy function (multipart upload)
- `GET /projects/:id/functions` — list functions
- `GET /projects/:id/logs` — query execution logs
- `POST /projects/:id/secrets` — set secret
- `GET /projects/:id/schema` — introspect DB schema (proxied to Data Engine)
- `GET /projects/:id/routes` — gateway route config

### Internal routes (service-to-service)
- `GET /internal/bundle` — Runtime fetches function bundles
- `GET /internal/routes` — Gateway fetches route snapshot
- `POST /internal/logs` — Services write execution spans

All internal routes require `X-Service-Token` header.

---

## Authentication

| Method | Use case |
|---|---|
| Firebase JWT | Dashboard, CLI after `flux auth login` |
| API key (`flux_*`) | Programmatic access, CI/CD |
| Service token | Internal service-to-service |

API keys are SHA-256 hashed, never stored in plaintext.

---

## AppState

```rust
pub struct AppState {
    pub db: PgPool,
    pub storage: StorageClient,    // R2/S3 for bundles + log archives
    pub config: AppConfig,
}
```

Shared across all routes via Axum state extraction.

---

## Key implementation details

### Deployments
- CLI uploads bundle as multipart form data
- Bundle stored in R2/S3, URL stored in Postgres
- Fallback: inline code in Postgres for small functions
- Each deploy creates an immutable deployment record with `code_sha`

### Secrets
- AES-256-GCM encrypted at rest
- Versioned — previous values recoverable
- Scoped to project

### Log pipeline
- Hot tier: `execution_records`, `execution_spans`, `execution_mutations` in Postgres (configurable retention)
- Cold tier: NDJSON archives in R2/S3 (after retention window)
- Async write — never blocks the execution hot path

---

## Configuration

| Env var | Default | Description |
|---|---|---|
| `DATABASE_URL` | — | Postgres connection string |
| `PORT` | `8080` | HTTP listen port |
| `INTERNAL_SERVICE_TOKEN` | — | Shared secret for service-to-service auth |
| `STORAGE_BUCKET` | — | R2/S3 bucket for bundles and logs |
| `FIREBASE_PROJECT_ID` | — | Firebase project for JWT verification |

---

*Source: `api/src/`. For the full architecture, see
[framework.md §4](framework.md#4-architecture).*
