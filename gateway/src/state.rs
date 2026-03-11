use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::mpsc;
use dashmap::DashMap;
use crate::cache::snapshot::GatewaySnapshot;
use crate::cache::query_cache::QueryCache;
use crate::clients::queue_client::QueueClient;
use crate::middleware::analytics::MetricRow;
use crate::middleware::query_guard::QueryGuardConfig;

#[derive(Clone)]
pub struct GatewayState {
    pub db_pool: PgPool,
    pub http_client: reqwest::Client,
    pub runtime_url: String,
    pub queue_client: QueueClient,
    pub data_engine_url: String,
    pub internal_service_token: String,
    pub snapshot: GatewaySnapshot,
    pub jwks_cache: crate::cache::jwks::JwksCache,
    /// Fluxbase API base URL — used to proxy SSE event streams.
    pub api_url: String,
    /// In-process edge cache for read-only data-engine query responses.
    pub query_cache: QueryCache,
    /// Bounded channel for fire-and-forget analytics writes.
    /// The drain worker (spawned once in main) drains this into `gateway_metrics`.
    /// Use `try_send` on the hot path — never block, never unbounded-spawn.
    pub metric_tx: mpsc::Sender<MetricRow>,

    // ── Gateway hardening (Improvements #1–#3) ──────────────────────────────
    /// Requests per second allowed per tenant before 429. From RATE_LIMIT_PER_SEC.
    pub rate_limit_per_sec: u32,
    /// Maximum concurrent in-flight queries per tenant. From MAX_CONCURRENT_PER_TENANT.
    pub max_concurrent_per_tenant: usize,
    /// Per-tenant semaphore pool — lazily allocated, one Arc<Semaphore> per tenant.
    pub tenant_semaphores: Arc<DashMap<String, Arc<tokio::sync::Semaphore>>>,
    /// Structural query validation limits applied before forwarding to data-engine.
    pub query_guard_config: QueryGuardConfig,
}

pub type SharedState = Arc<GatewayState>;
