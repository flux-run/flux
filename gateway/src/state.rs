use sqlx::PgPool;
use std::sync::Arc;
use crate::cache::snapshot::GatewaySnapshot;

#[derive(Clone)]
pub struct GatewayState {
    pub db_pool: PgPool,
    pub http_client: reqwest::Client,
    pub runtime_url: String,
    pub internal_service_token: String,
    pub snapshot: GatewaySnapshot,
    pub jwks_cache: crate::cache::jwks::JwksCache,
}

pub type SharedState = Arc<GatewayState>;
