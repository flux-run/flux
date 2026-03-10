use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::mpsc;
use crate::cache::snapshot::GatewaySnapshot;
use crate::cache::query_cache::QueryCache;
use crate::clients::queue_client::QueueClient;
use crate::middleware::analytics::MetricRow;

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
}

pub type SharedState = Arc<GatewayState>;
