use uuid::Uuid;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct RouteRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub tenant_id: Uuid,
    pub path: String,
    pub method: String,
    pub function_id: Uuid,
    pub is_async: bool,
    pub auth_type: String,
    pub cors_enabled: bool,
    pub rate_limit: Option<i32>,
    pub jwks_url: Option<String>,
    pub jwt_audience: Option<String>,
    pub jwt_issuer: Option<String>,
    pub json_schema: Option<serde_json::Value>,
    pub cors_origins: Option<Vec<String>>,
    pub cors_headers: Option<Vec<String>>,
}
