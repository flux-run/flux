//! Typed route registry — single source of truth for every Flux API endpoint.
//!
//! Each constant binds **three things together** at compile time:
//!   - The HTTP method
//!   - The URL path
//!   - The request/response types
//!
//! Both the API (route registration in `app.rs`) and the CLI (HTTP call sites)
//! reference these constants. A path rename or type change causes a compile
//! error in any crate that drifts from the contract.
//!
//! # Pattern
//! ```rust
//! // API — app.rs
//! .route(routes::db::MIGRATE_APPLY.path, post(handler))
//!
//! // CLI
//! client.post(routes::db::MIGRATE_APPLY.url(&base))
//!       .json(&MigrationApplyRequest { .. })
//!       .send().await?;
//! ```
//!
//! # Adding a new route
//! 1. Define request/response types in the appropriate `api_contract` module.
//! 2. Add a `pub const` here.
//! 3. The API handler and CLI call site get a compile error until they update.

use std::marker::PhantomData;

// ── Route<Req, Resp> ──────────────────────────────────────────────────────────

/// A compile-time binding of (method, path) → (Req, Resp).
///
/// `Req` is the JSON request body type (`()` for GET / no-body requests).
/// `Resp` is the JSON response body type.
///
/// Zero-sized at runtime — all information lives in `const` fields or is
/// encoded in the type parameters.
pub struct Route<Req, Resp> {
    pub method: &'static str,
    pub path:   &'static str,
    _req:  PhantomData<fn(Req) -> Resp>,
}

impl<Req, Resp> Route<Req, Resp> {
    pub const fn new(method: &'static str, path: &'static str) -> Self {
        Self { method, path, _req: PhantomData }
    }

    /// Build the full URL: `routes::functions::LIST.url("http://localhost:4000")`
    /// → `"http://localhost:4000/functions"`.
    pub fn url(&self, base: &str) -> String {
        format!("{}{}", base.trim_end_matches('/'), self.path)
    }

    /// Build a full URL substituting `{param}` placeholders.
    ///
    /// ```rust
    /// routes::gateway::ROUTES_GET.url_with(&base, &[("id", route_id.as_str())])
    /// // → "http://host/gateway/routes/abc-123"
    /// ```
    pub fn url_with(&self, base: &str, params: &[(&str, &str)]) -> String {
        let mut path = self.path.to_string();
        for (key, val) in params {
            path = path.replace(&format!("{{{}}}", key), val);
        }
        format!("{}{}", base.trim_end_matches('/'), path)
    }

    /// Return the path with the given static prefix stripped — for use inside
    /// an axum `.nest(prefix, ...)` router so the outer prefix isn't doubled.
    ///
    /// ```rust
    /// // routes::internal::BUNDLE_GET.path == "/internal/bundle"
    /// .route(routes::internal::BUNDLE_GET.under("/internal"), get(handler))
    /// // axum sees "/bundle", the nest adds "/internal" ⇒ "/internal/bundle" ✓
    /// ```
    pub fn under(&self, prefix: &str) -> &'static str {
        self.path.strip_prefix(prefix).unwrap_or(self.path)
    }
}

// ── Internal routes (service-token protected, /internal/*) ───────────────────

pub mod internal {
    use super::Route;
    use serde_json::Value;
    use crate::db_migrate::{MigrateRequest, MigrateResponse};
    use crate::schema::SchemaManifest;

    /// `GET /internal/secrets`
    pub const SECRETS_GET:       Route<(), Value>                   = Route::new("GET",  "/internal/secrets");
    /// `GET /internal/bundle`
    pub const BUNDLE_GET:        Route<(), Value>                   = Route::new("GET",  "/internal/bundle");
    /// `GET /internal/introspect`
    pub const INTROSPECT:        Route<(), Value>                   = Route::new("GET",  "/internal/introspect");
    /// `GET /internal/introspect/manifest`
    pub const MANIFEST:          Route<(), Value>                   = Route::new("GET",  "/internal/introspect/manifest");
    /// `POST /internal/db/migrate` — single migration file (used by `flux db push`)
    pub const DB_MIGRATE:        Route<MigrateRequest, MigrateResponse> = Route::new("POST", "/internal/db/migrate");
    /// `POST /internal/db/schema` — push table schema manifest
    pub const DB_SCHEMA:         Route<SchemaManifest, Value>       = Route::new("POST", "/internal/db/schema");
    /// `POST /internal/logs`
    pub const LOGS_CREATE:       Route<Value, Value>                = Route::new("POST", "/internal/logs");
    /// `GET /internal/logs`
    pub const LOGS_LIST:         Route<(), Value>                   = Route::new("GET",  "/internal/logs");
    /// `POST /internal/network-calls` — ingest an outbound network call recorded by the runtime
    pub const NETWORK_CALLS_CREATE: Route<Value, Value>             = Route::new("POST", "/internal/network-calls");
    /// `GET /internal/functions/resolve`
    pub const FUNCTIONS_RESOLVE: Route<(), Value>                   = Route::new("GET",  "/internal/functions/resolve");
    /// `POST /internal/cache/invalidate`
    pub const CACHE_INVALIDATE:  Route<Value, Value>                = Route::new("POST", "/internal/cache/invalidate");
    /// `GET /internal/routes` — gateway snapshot for the gateway service
    pub const ROUTES_GET:        Route<(), Value>                   = Route::new("GET",  "/internal/routes");
    /// `GET /internal/metrics` — Prometheus metrics endpoint (gateway)
    pub const METRICS:           Route<(), Value>                   = Route::new("GET",  "/internal/metrics");
}

// ── Functions ─────────────────────────────────────────────────────────────────

pub mod functions {
    use super::Route;
    use serde_json::Value;
    use crate::functions::CreateFunctionPayload;

    /// `GET /functions`
    pub const LIST:   Route<(), Value>                    = Route::new("GET",    "/functions");
    /// `POST /functions`
    pub const CREATE: Route<CreateFunctionPayload, Value> = Route::new("POST",   "/functions");
    /// `GET /functions/{id}`
    pub const GET:    Route<(), Value>                    = Route::new("GET",    "/functions/{id}");
    /// `DELETE /functions/{id}`
    pub const DELETE: Route<(), Value>                    = Route::new("DELETE", "/functions/{id}");
    /// `POST /functions/deploy` — multipart bundle upload
    pub const DEPLOY: Route<(), Value>                    = Route::new("POST",   "/functions/deploy");

    // ── Version management ────────────────────────────────────────────────────

    /// `GET /functions/{name}/deployments`
    pub const DEPLOYMENTS_LIST:     Route<(), Value>      = Route::new("GET",  "/functions/{name}/deployments");
    /// `GET /functions/{name}/deployments/{version}`
    pub const DEPLOYMENTS_GET:      Route<(), Value>      = Route::new("GET",  "/functions/{name}/deployments/{version}");
    /// `POST /functions/{name}/deployments/{version}/activate`
    pub const DEPLOYMENTS_ACTIVATE: Route<(), Value>      = Route::new("POST", "/functions/{name}/deployments/{version}/activate");
    /// `POST /functions/{name}/deployments/{version}/promote`
    pub const DEPLOYMENTS_PROMOTE:  Route<Value, Value>   = Route::new("POST", "/functions/{name}/deployments/{version}/promote");
    /// `GET /functions/{name}/deployments/diff`
    pub const DEPLOYMENTS_DIFF:     Route<(), Value>      = Route::new("GET",  "/functions/{name}/deployments/diff");
}

// ── Deployments ───────────────────────────────────────────────────────────────

pub mod deployments {
    use super::Route;
    use serde_json::Value;
    use crate::deployments::{CreateDeploymentPayload, CreateProjectDeploymentPayload};

    /// `POST /deployments`
    pub const CREATE:           Route<CreateDeploymentPayload, Value>        = Route::new("POST",   "/deployments");
    /// `GET /deployments/list/{id}`
    pub const LIST:             Route<(), Value>                             = Route::new("GET",    "/deployments/list/{id}");
    /// `POST /deployments/{id}/activate/{version}`
    pub const ACTIVATE:         Route<(), Value>                             = Route::new("POST",   "/deployments/{id}/activate/{version}");
    /// `GET /deployments/hashes`
    pub const HASHES:           Route<(), Value>                             = Route::new("GET",    "/deployments/hashes");
    /// `GET /deployments/project`
    pub const PROJECT_LIST:     Route<(), Value>                             = Route::new("GET",    "/deployments/project");
    /// `POST /deployments/project`
    pub const PROJECT_CREATE:   Route<CreateProjectDeploymentPayload, Value> = Route::new("POST",   "/deployments/project");
    /// `POST /deployments/project/{id}/rollback`
    pub const PROJECT_ROLLBACK: Route<(), Value>                             = Route::new("POST",   "/deployments/project/{id}/rollback");
}

// ── Secrets ───────────────────────────────────────────────────────────────────

pub mod secrets {
    use super::Route;
    use serde_json::Value;
    use crate::secrets::{SecretResponse, CreateSecretRequest, UpdateSecretRequest};

    /// `GET /secrets`
    pub const LIST:   Route<(), Vec<SecretResponse>>             = Route::new("GET",    "/secrets");
    /// `POST /secrets`
    pub const CREATE: Route<CreateSecretRequest, Value>          = Route::new("POST",   "/secrets");
    /// `PUT /secrets/{key}`
    pub const UPDATE: Route<UpdateSecretRequest, Value>          = Route::new("PUT",    "/secrets/{key}");
    /// `DELETE /secrets/{key}`
    pub const DELETE: Route<(), Value>                           = Route::new("DELETE", "/secrets/{key}");
}

// ── Logs & Traces ─────────────────────────────────────────────────────────────

pub mod logs {
    use super::Route;
    use serde_json::Value;

    /// `GET /logs`
    pub const LIST:         Route<(), Value> = Route::new("GET",  "/logs");
    /// `GET /traces`
    pub const TRACES_LIST:  Route<(), Value> = Route::new("GET",  "/traces");
    /// `GET /traces/{request_id}`
    pub const TRACE_GET:    Route<(), Value> = Route::new("GET",  "/traces/{request_id}");
    /// `POST /traces/{request_id}/replay`
    pub const TRACE_REPLAY:    Route<Value, Value> = Route::new("POST", "/traces/{request_id}/replay");
    /// `GET /traces/errors/summary` — per-function error summary
    pub const ERRORS_SUMMARY:  Route<(), Value> = Route::new("GET",  "/traces/errors/summary");
}

// ── Gateway routes & middleware ───────────────────────────────────────────────

pub mod gateway {
    use super::Route;
    use serde_json::Value;
    use crate::gateway::{
        RouteRow, RouteFullRow,
        CreateRoutePayload, UpdateRoutePayload,
        RateLimitPayload, CorsPayload, MiddlewareCreatePayload,
    };

    /// `GET /gateway/routes`
    pub const ROUTES_LIST:       Route<(), Vec<RouteRow>>            = Route::new("GET",    "/gateway/routes");
    /// `POST /gateway/routes`
    pub const ROUTES_CREATE:     Route<CreateRoutePayload, RouteRow> = Route::new("POST",   "/gateway/routes");
    /// `GET /gateway/routes/{id}`
    pub const ROUTES_GET:        Route<(), RouteFullRow>             = Route::new("GET",    "/gateway/routes/{id}");
    /// `PATCH /gateway/routes/{id}`
    pub const ROUTES_UPDATE:     Route<UpdateRoutePayload, RouteRow> = Route::new("PATCH",  "/gateway/routes/{id}");
    /// `DELETE /gateway/routes/{id}`
    pub const ROUTES_DELETE:     Route<(), Value>                    = Route::new("DELETE", "/gateway/routes/{id}");
    /// `POST /gateway/middleware`
    pub const MIDDLEWARE_CREATE: Route<MiddlewareCreatePayload, Value> = Route::new("POST", "/gateway/middleware");
    /// `DELETE /gateway/middleware/{route}/{type}`
    pub const MIDDLEWARE_DELETE: Route<(), Value>                    = Route::new("DELETE", "/gateway/middleware/{route}/{type}");
    /// `PUT /gateway/routes/{id}/rate-limit`
    pub const RATE_LIMIT_SET:    Route<RateLimitPayload, Value>      = Route::new("PUT",    "/gateway/routes/{id}/rate-limit");
    /// `DELETE /gateway/routes/{id}/rate-limit`
    pub const RATE_LIMIT_DELETE: Route<(), Value>                    = Route::new("DELETE", "/gateway/routes/{id}/rate-limit");
    /// `GET /gateway/routes/{id}/cors`
    pub const CORS_GET:          Route<(), Value>                    = Route::new("GET",    "/gateway/routes/{id}/cors");
    /// `PUT /gateway/routes/{id}/cors`
    pub const CORS_SET:          Route<CorsPayload, Value>           = Route::new("PUT",    "/gateway/routes/{id}/cors");
}

// ── Schema / SDK / OpenAPI ────────────────────────────────────────────────────

pub mod sdk {
    use super::Route;
    use serde_json::Value;

    /// `GET /schema/graph`
    pub const SCHEMA_GRAPH: Route<(), Value> = Route::new("GET", "/schema/graph");
    /// `GET /sdk/schema`
    pub const SDK_SCHEMA:   Route<(), Value> = Route::new("GET", "/sdk/schema");
    /// `GET /sdk/typescript`
    pub const SDK_TS:       Route<(), Value> = Route::new("GET", "/sdk/typescript");
    /// `GET /sdk/manifest`
    pub const MANIFEST:     Route<(), Value> = Route::new("GET", "/sdk/manifest");
    /// `GET /openapi.json`
    pub const OPENAPI:      Route<(), Value> = Route::new("GET", "/openapi.json");
    /// `GET /spec`
    pub const SPEC:         Route<(), Value> = Route::new("GET", "/spec");
    /// `GET /openapi/ui` — Swagger UI HTML page
    pub const OPENAPI_UI:   Route<(), Value> = Route::new("GET", "/openapi/ui");
}

// ── Database / Migrations ─────────────────────────────────────────────────────

pub mod db {
    use super::Route;
    use serde_json::Value;
    use crate::db_migrate::{
        MigrateRequest, MigrateResponse,
        MigrationApplyRequest, MigrationApplyResponse,
        MigrationRollbackRequest, MigrationRollbackResponse,
        MigrationStatusResponse,
    };

    // ── Migrations ────────────────────────────────────────────────────────────

    /// `POST /db/migrate` — apply a single migration file (internal, used by `flux db push`)
    pub const MIGRATE_SINGLE:   Route<MigrateRequest, MigrateResponse>                    = Route::new("POST", "/db/migrate");
    /// `POST /db/migrations/apply` — apply all pending migrations
    pub const MIGRATE_APPLY:    Route<MigrationApplyRequest, MigrationApplyResponse>      = Route::new("POST", "/db/migrations/apply");
    /// `POST /db/migrations/rollback` — roll back the last applied migration
    pub const MIGRATE_ROLLBACK: Route<MigrationRollbackRequest, MigrationRollbackResponse> = Route::new("POST", "/db/migrations/rollback");
    /// `GET /db/migrations` — list all applied migrations
    pub const MIGRATE_STATUS:   Route<(), MigrationStatusResponse>                        = Route::new("GET",  "/db/migrations");

    // ── Data-engine proxy (these hit the /db/{*path} wildcard in app.rs) ─────
    // CLI calls these directly; app.rs proxies them to the data-engine service.

    /// `GET /db/databases`
    pub const DATABASES_LIST:   Route<(), Value>      = Route::new("GET",  "/db/databases");
    /// `POST /db/databases`
    pub const DATABASES_CREATE: Route<Value, Value>   = Route::new("POST", "/db/databases");
    /// `GET /db/tables/{database}`
    pub const TABLES_LIST:      Route<(), Value>      = Route::new("GET",  "/db/tables/{database}");
    /// `POST /db/tables`
    pub const TABLES_CREATE:    Route<Value, Value>   = Route::new("POST", "/db/tables");
    /// `GET /db/diff`
    pub const DIFF:             Route<(), Value>      = Route::new("GET",  "/db/diff");
    /// `POST /db/query`
    pub const QUERY:            Route<Value, Value>   = Route::new("POST", "/db/query");
    /// `GET /db/connection`
    pub const CONNECTION:       Route<(), Value>      = Route::new("GET",  "/db/connection");
    /// `GET /db/history/{database}/{table}`
    pub const HISTORY:          Route<(), Value>      = Route::new("GET",  "/db/history/{database}/{table}");
    /// `GET /db/blame/{database}/{table}`
    pub const BLAME:            Route<(), Value>      = Route::new("GET",  "/db/blame/{database}/{table}");
    /// `GET /db/mutations`
    pub const MUTATIONS:        Route<(), Value>      = Route::new("GET",  "/db/mutations");
    /// `GET /db/network-calls` — outbound HTTP calls recorded during execution
    pub const NETWORK_CALLS:    Route<(), Value>      = Route::new("GET",  "/db/network-calls");
    /// `POST /db/explain`
    pub const EXPLAIN:          Route<Value, Value>   = Route::new("POST", "/db/explain");
    /// `POST /db/sql` — raw SQL passthrough
    pub const SQL:              Route<Value, Value>   = Route::new("POST", "/db/sql");
    /// `DELETE /db/databases/{name}`
    pub const DATABASES_DELETE: Route<(), Value>      = Route::new("DELETE", "/db/databases/{name}");
    /// `DELETE /db/tables/{database}/{table}`
    pub const TABLES_DELETE:    Route<(), Value>      = Route::new("DELETE", "/db/tables/{database}/{table}");
    /// `GET /db/relationships`
    pub const RELATIONSHIPS_LIST:   Route<(), Value>      = Route::new("GET",    "/db/relationships");
    /// `POST /db/relationships`
    pub const RELATIONSHIPS_CREATE: Route<Value, Value>   = Route::new("POST",   "/db/relationships");
    /// `DELETE /db/relationships/{id}`
    pub const RELATIONSHIPS_DELETE: Route<(), Value>      = Route::new("DELETE",  "/db/relationships/{id}");
    /// `GET /db/cron`
    pub const CRON_LIST:    Route<(), Value>      = Route::new("GET",    "/db/cron");
    /// `POST /db/cron`
    pub const CRON_CREATE:  Route<Value, Value>   = Route::new("POST",   "/db/cron");
    /// `PATCH /db/cron/{id}`
    pub const CRON_UPDATE:  Route<Value, Value>   = Route::new("PATCH",  "/db/cron/{id}");
    /// `DELETE /db/cron/{id}`
    pub const CRON_DELETE:  Route<(), Value>      = Route::new("DELETE", "/db/cron/{id}");
    /// `POST /db/cron/{id}/trigger`
    pub const CRON_TRIGGER: Route<(), Value>      = Route::new("POST",   "/db/cron/{id}/trigger");
    /// `GET /db/replay/{database}`
    pub const REPLAY:       Route<(), Value>      = Route::new("GET",    "/db/replay/{database}");
    /// `GET /db/schema` — introspect schema
    pub const SCHEMA:       Route<(), Value>      = Route::new("GET",    "/db/schema");
    /// `GET /db/debug` — engine debug info
    pub const DEBUG:        Route<(), Value>      = Route::new("GET",    "/db/debug");
}

// ── API keys ──────────────────────────────────────────────────────────────────

pub mod api_keys {
    use super::Route;
    use serde_json::Value;
    use crate::api_keys::{ApiKeyRow, CreateApiKeyPayload};

    /// `GET /api-keys`
    pub const LIST:   Route<(), Vec<ApiKeyRow>>               = Route::new("GET",    "/api-keys");
    /// `POST /api-keys`
    pub const CREATE: Route<CreateApiKeyPayload, Value>       = Route::new("POST",   "/api-keys");
    /// `DELETE /api-keys/{id}`
    pub const DELETE: Route<(), Value>                        = Route::new("DELETE", "/api-keys/{id}");
    /// `POST /api-keys/{id}/rotate`
    pub const ROTATE: Route<(), Value>                        = Route::new("POST",   "/api-keys/{id}/rotate");
}

// ── Execution records ─────────────────────────────────────────────────────────

pub mod records {
    use super::Route;
    use serde_json::Value;

    /// `GET /records/export`
    pub const EXPORT: Route<(), Value> = Route::new("GET",    "/records/export");
    /// `GET /records/count`
    pub const COUNT:  Route<(), Value> = Route::new("GET",    "/records/count");
    /// `DELETE /records/prune`
    pub const PRUNE:  Route<(), Value> = Route::new("DELETE", "/records/prune");
}

// ── Monitor / alerts ──────────────────────────────────────────────────────────

pub mod monitor {
    use super::Route;
    use serde_json::Value;
    use crate::monitor::CreateAlertPayload;

    /// `GET /monitor/status`
    pub const STATUS:        Route<(), Value>                 = Route::new("GET",    "/monitor/status");
    /// `GET /monitor/metrics`
    pub const METRICS:       Route<(), Value>                 = Route::new("GET",    "/monitor/metrics");
    /// `GET /monitor/alerts`
    pub const ALERTS_LIST:   Route<(), Value>                 = Route::new("GET",    "/monitor/alerts");
    /// `POST /monitor/alerts`
    pub const ALERTS_CREATE: Route<CreateAlertPayload, Value> = Route::new("POST",   "/monitor/alerts");
    /// `DELETE /monitor/alerts/{id}`
    pub const ALERTS_DELETE: Route<(), Value>                 = Route::new("DELETE", "/monitor/alerts/{id}");
}

// ── Events ────────────────────────────────────────────────────────────────────

pub mod events {
    use super::Route;
    use serde_json::Value;
    use crate::events::{
        PublishEventPayload, EventRow,
        EventSubscriptionRow, CreateSubscriptionPayload,
    };

    /// `POST /events`
    pub const PUBLISH:              Route<PublishEventPayload, EventRow>               = Route::new("POST",   "/events");
    /// `GET /events/subscriptions`
    pub const SUBSCRIPTIONS_LIST:   Route<(), Vec<EventSubscriptionRow>>               = Route::new("GET",    "/events/subscriptions");
    /// `POST /events/subscriptions`
    pub const SUBSCRIPTIONS_CREATE: Route<CreateSubscriptionPayload, EventSubscriptionRow> = Route::new("POST", "/events/subscriptions");
    /// `DELETE /events/subscriptions/{id}`
    pub const SUBSCRIPTIONS_DELETE: Route<(), Value>                                   = Route::new("DELETE", "/events/subscriptions/{id}");
    /// `GET /events/history` — event history filtered by type/since
    pub const HISTORY:              Route<(), Value>                                   = Route::new("GET",    "/events/history");
}

// ── Queues ────────────────────────────────────────────────────────────────────

pub mod queues {
    use super::Route;
    use serde_json::Value;
    use crate::queue::{
        QueueConfigRow, CreateQueuePayload,
        PublishMessagePayload,
        QueueBindingRow, CreateBindingPayload,
        DeadLetterJobRow,
    };

    /// `GET /queues`
    pub const LIST:            Route<(), Vec<QueueConfigRow>>              = Route::new("GET",    "/queues");
    /// `POST /queues`
    pub const CREATE:          Route<CreateQueuePayload, QueueConfigRow>   = Route::new("POST",   "/queues");
    /// `GET /queues/{name}`
    pub const GET:             Route<(), Value>                            = Route::new("GET",    "/queues/{name}");
    /// `DELETE /queues/{name}`
    pub const DELETE:          Route<(), Value>                            = Route::new("DELETE", "/queues/{name}");
    /// `POST /queues/{name}/messages`
    pub const PUBLISH:         Route<PublishMessagePayload, Value>         = Route::new("POST",   "/queues/{name}/messages");
    /// `GET /queues/{name}/bindings`
    pub const BINDINGS_LIST:   Route<(), Vec<QueueBindingRow>>             = Route::new("GET",    "/queues/{name}/bindings");
    /// `POST /queues/{name}/bindings`
    pub const BINDINGS_CREATE: Route<CreateBindingPayload, QueueBindingRow> = Route::new("POST",  "/queues/{name}/bindings");
    /// `POST /queues/{name}/purge`
    pub const PURGE:           Route<(), Value>                            = Route::new("POST",   "/queues/{name}/purge");
    /// `GET /queues/{name}/dlq`
    pub const DLQ_LIST:        Route<(), Vec<DeadLetterJobRow>>            = Route::new("GET",    "/queues/{name}/dlq");
    /// `POST /queues/{name}/dlq/replay`
    pub const DLQ_REPLAY:      Route<(), Value>                            = Route::new("POST",   "/queues/{name}/dlq/replay");
}

// ── Schedules ─────────────────────────────────────────────────────────────────

pub mod schedules {
    use super::Route;
    use serde_json::Value;
    use crate::schedules::{CronJobRow, CreateSchedulePayload};

    /// `GET /schedules`
    pub const LIST:    Route<(), Vec<CronJobRow>>               = Route::new("GET",    "/schedules");
    /// `POST /schedules`
    pub const CREATE:  Route<CreateSchedulePayload, CronJobRow> = Route::new("POST",   "/schedules");
    /// `DELETE /schedules/{name}`
    pub const DELETE:  Route<(), Value>                         = Route::new("DELETE", "/schedules/{name}");
    /// `POST /schedules/{name}/pause`
    pub const PAUSE:   Route<(), Value>                         = Route::new("POST",   "/schedules/{name}/pause");
    /// `POST /schedules/{name}/resume`
    pub const RESUME:  Route<(), Value>                         = Route::new("POST",   "/schedules/{name}/resume");
    /// `POST /schedules/{name}/run`
    pub const RUN:     Route<(), Value>                         = Route::new("POST",   "/schedules/{name}/run");
    /// `GET /schedules/{name}/history`
    pub const HISTORY: Route<(), Value>                         = Route::new("GET",    "/schedules/{name}/history");
}

// ── Environments ──────────────────────────────────────────────────────────────

pub mod environments {
    use super::Route;
    use serde_json::Value;
    use crate::environments::{EnvironmentRow, CreateEnvPayload, CloneEnvPayload};

    /// `GET /environments`
    pub const LIST:   Route<(), Vec<EnvironmentRow>>             = Route::new("GET",    "/environments");
    /// `POST /environments`
    pub const CREATE: Route<CreateEnvPayload, EnvironmentRow>    = Route::new("POST",   "/environments");
    /// `POST /environments/clone`
    pub const CLONE:  Route<CloneEnvPayload, EnvironmentRow>     = Route::new("POST",   "/environments/clone");
    /// `DELETE /environments/{name}`
    pub const DELETE: Route<(), Value>                           = Route::new("DELETE", "/environments/{name}");
}

// ── Gateway config / route sync ───────────────────────────────────────────────

pub mod config {
    use super::Route;
    use serde_json::Value;
    use crate::gateway::SyncRoutesPayload;

    /// `GET /routes`
    pub const LIST: Route<(), Value>                = Route::new("GET",  "/routes");
    /// `POST /routes/sync`
    pub const SYNC: Route<SyncRoutesPayload, Value> = Route::new("POST", "/routes/sync");
}

// ── Server-Sent Events live streams ───────────────────────────────────────────

pub mod stream {
    use super::Route;
    use serde_json::Value;

    /// `GET /stream/events`
    pub const EVENTS:     Route<(), Value> = Route::new("GET", "/stream/events");
    /// `GET /stream/executions`
    pub const EXECUTIONS: Route<(), Value> = Route::new("GET", "/stream/executions");
    /// `GET /stream/mutations`
    pub const MUTATIONS:  Route<(), Value> = Route::new("GET", "/stream/mutations");
}
// ── Auth ──────────────────────────────────────────────────────────────────────

pub mod auth {
    use super::Route;
    use serde_json::Value;

    /// `GET /auth/status`
    pub const STATUS:       Route<(), Value>     = Route::new("GET",    "/auth/status");
    /// `POST /auth/setup`
    pub const SETUP:        Route<Value, Value>  = Route::new("POST",   "/auth/setup");
    /// `POST /auth/login`
    pub const LOGIN:        Route<Value, Value>  = Route::new("POST",   "/auth/login");
    /// `POST /auth/logout`
    pub const LOGOUT:       Route<(), Value>     = Route::new("POST",   "/auth/logout");
    /// `GET /auth/me`
    pub const ME:           Route<(), Value>     = Route::new("GET",    "/auth/me");
    /// `GET /auth/users`
    pub const USERS_LIST:   Route<(), Value>     = Route::new("GET",    "/auth/users");
    /// `POST /auth/users`
    pub const USERS_CREATE: Route<Value, Value>  = Route::new("POST",   "/auth/users");
    /// `DELETE /auth/users/{id}`
    pub const USERS_DELETE: Route<(), Value>     = Route::new("DELETE", "/auth/users/{id}");
}

// ── Health / Version ──────────────────────────────────────────────────────────

pub mod health {
    use super::Route;
    use serde_json::Value;

    /// `GET /health`
    pub const HEALTH:    Route<(), Value> = Route::new("GET", "/health");
    /// `GET /readiness`
    pub const READINESS: Route<(), Value> = Route::new("GET", "/readiness");
    /// `GET /version`
    pub const VERSION:   Route<(), Value> = Route::new("GET", "/version");
}

// ── Tenants ───────────────────────────────────────────────────────────────────

pub mod tenants {
    use super::Route;
    use serde_json::Value;

    /// `GET /tenants`
    pub const LIST:   Route<(), Value>    = Route::new("GET",  "/tenants");
    /// `POST /tenants`
    pub const CREATE: Route<Value, Value> = Route::new("POST", "/tenants");
}

// ── Queue jobs (queue service API) ───────────────────────────────────────────

pub mod jobs {
    use super::Route;
    use serde_json::Value;

    /// `GET /jobs`
    pub const LIST:   Route<(), Value>    = Route::new("GET",    "/jobs");
    /// `POST /jobs`
    pub const CREATE: Route<Value, Value> = Route::new("POST",   "/jobs");
    /// `GET /jobs/stats`
    pub const STATS:  Route<(), Value>    = Route::new("GET",    "/jobs/stats");
    /// `GET /jobs/{id}`
    pub const GET:    Route<(), Value>    = Route::new("GET",    "/jobs/{id}");
    /// `DELETE /jobs/{id}` — cancel job
    pub const CANCEL: Route<(), Value>    = Route::new("DELETE", "/jobs/{id}");
    /// `POST /jobs/{id}/retry`
    pub const RETRY:  Route<(), Value>    = Route::new("POST",   "/jobs/{id}/retry");
}

// ── Execution (runtime service) ──────────────────────────────────────────────

pub mod execution {
    use super::Route;
    use serde_json::Value;

    /// `POST /execute` — runtime function execution endpoint
    pub const EXECUTE:           Route<Value, Value> = Route::new("POST", "/execute");
    /// `POST /flux/dev/invoke/{name}` — dev-mode invocation (server crate)
    pub const DEV_INVOKE:        Route<Value, Value> = Route::new("POST", "/flux/dev/invoke/{name}");

    // ── Execution guards (api service rejects these paths) ────────────────
    /// `POST /run` — blocked on API, belongs to runtime
    pub const RUN:               Route<Value, Value> = Route::new("POST", "/run");
    /// `POST /run/{*path}`
    pub const RUN_WILDCARD:      Route<Value, Value> = Route::new("POST", "/run/{*path}");
    /// `POST /invoke`
    pub const INVOKE:            Route<Value, Value> = Route::new("POST", "/invoke");
    /// `POST /invoke/{*path}`
    pub const INVOKE_WILDCARD:   Route<Value, Value> = Route::new("POST", "/invoke/{*path}");
    /// `POST /execute/{*path}`
    pub const EXECUTE_WILDCARD:  Route<Value, Value> = Route::new("POST", "/execute/{*path}");
    /// `POST /functions/{name}/run`
    pub const FUNCTION_RUN:      Route<Value, Value> = Route::new("POST", "/functions/{name}/run");
    /// `POST /functions/{name}/invoke`
    pub const FUNCTION_INVOKE:   Route<Value, Value> = Route::new("POST", "/functions/{name}/invoke");
}

// ── Proxy wildcard paths ──────────────────────────────────────────────────────
// These are axum catch-all wildcards — not typed request/response contracts,
// but centralized here so the path strings have a single source of truth.

pub mod proxy {
    use super::Route;
    use serde_json::Value;

    /// `ANY /db/{*path}` — proxied from API to data-engine
    pub const DB:               Route<Value, Value> = Route::new("ANY", "/db/{*path}");
    /// `ANY /files/{*path}` — proxied from API to data-engine
    pub const FILES:            Route<Value, Value> = Route::new("ANY", "/files/{*path}");
    /// `ANY /{*path}` — gateway dispatch wildcard (all user function calls)
    pub const GATEWAY_DISPATCH: Route<Value, Value> = Route::new("ANY", "/{*path}");
}

