//! Shared gateway state — injected into every handler via Axum `State`.
//!
//! Fields are intentionally minimal: only what at least one handler reads.
//! Add a field here only when it is needed across multiple handlers or
//! it is expensive to construct per-request.
use sqlx::PgPool;
use std::sync::Arc;
use crate::auth::JwksCache;
use crate::snapshot::GatewaySnapshot;
use job_contract::dispatch::RuntimeDispatch;

#[derive(Clone)]
pub struct GatewayState {
    /// Database pool — API-key validation + trace root writes.
    pub db_pool: PgPool,
    /// Runtime dispatch — abstracts over HTTP (multi-process) or in-process
    /// (server crate) execution of user functions.
    pub runtime: Arc<dyn RuntimeDispatch>,
    /// In-memory route snapshot.
    pub snapshot: GatewaySnapshot,
    /// JWKS key cache for JWT verification.
    pub jwks_cache: JwksCache,
    /// Hard limit on request body bytes (returns 413 above this).
    pub max_request_size_bytes: usize,
    /// Per-route default rate limit (requests / second).
    pub rate_limit_per_sec: u32,
    /// When true, skip auth — `flux dev` local stack.
    pub local_mode: bool,
}

pub type SharedState = Arc<GatewayState>;
