//! Shared gateway state — injected into every handler via Axum `State`.
//!
//! [`GatewayState`] is constructed once at startup and cheaply cloned (it is
//! `Arc`-wrapped as [`SharedState`]).  Fields are intentionally minimal:
//! only add a field here when it is needed across multiple handlers or is
//! expensive to construct per-request.
//!
//! ## Field absence contract
//!
//! Every field in [`GatewayState`] is required.  There are no `Option`s
//! at this level — if a field is absent the process panics at startup
//! (e.g., missing `DATABASE_URL`) rather than returning 500 at request time.
use sqlx::PgPool;
use std::sync::Arc;
use crate::auth::JwksCache;
use crate::snapshot::GatewaySnapshot;
use job_contract::dispatch::RuntimeDispatch;

#[derive(Clone)]
pub struct GatewayState {
    /// Postgres connection pool — used by API-key validation and trace-root
    /// writes.  Missing: process panics at startup (`DATABASE_URL` required).
    pub db_pool: PgPool,
    /// Runtime dispatch — abstracts over HTTP (multi-process) or in-process
    /// (server crate) execution of user functions.  Gateway always depends on
    /// this trait, never on `HttpRuntimeDispatch` directly (DIP).
    pub runtime: Arc<dyn RuntimeDispatch>,
    /// In-memory route snapshot — refreshed via Postgres LISTEN/NOTIFY.
    /// Missing or empty: gateway returns 503 until the first refresh succeeds.
    pub snapshot: GatewaySnapshot,
    /// JWKS key cache for JWT verification — entries expire per the JWKS TTL.
    /// Missing: every JWT route would fail on first request and re-fetch.
    pub jwks_cache: JwksCache,
    /// Hard limit on request body bytes — requests above this get HTTP 413.
    /// Defaults to 10 MB (`MAX_REQUEST_SIZE_BYTES`).
    pub max_request_size_bytes: usize,
    /// Per-route default rate limit (requests / second).
    /// Overridden per-route when `routes.rate_limit IS NOT NULL`.
    pub rate_limit_per_sec: u32,
    /// When `true`, skip tenant resolution and inject a fixed dev identity.
    /// Must be `false` in any internet-facing deployment.
    pub local_mode: bool,
}

pub type SharedState = Arc<GatewayState>;
