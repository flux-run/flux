use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct GatewayState {
    pub db_pool: PgPool,
    pub http_client: reqwest::Client,
    pub runtime_url: String,
    pub internal_service_token: String,
}

pub type SharedState = Arc<GatewayState>;
