use sqlx::PgPool;
use std::sync::Arc;
use dashmap::DashMap;
use uuid::Uuid;
use crate::services::route_lookup::RouteRecord;

#[derive(Clone)]
pub struct GatewayState {
    pub db_pool: PgPool,
    pub http_client: reqwest::Client,
    pub runtime_url: String,
    pub internal_service_token: String,
    pub tenant_cache: DashMap<String, Uuid>,
    pub route_cache: DashMap<(Uuid, String, String), RouteRecord>,
}

pub type SharedState = Arc<GatewayState>;
