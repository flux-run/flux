use sqlx::PgPool;
use std::sync::Arc;
use crate::cache::snapshot::GatewaySnapshot;
use crate::clients::queue_client::QueueClient;

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
}

pub type SharedState = Arc<GatewayState>;
