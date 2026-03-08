use sqlx::PgPool;
use uuid::Uuid;
use serde::Serialize;

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct RouteRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub tenant_id: Uuid,
    pub path: String,
    pub method: String,
    pub function_id: Uuid,
    pub auth_type: String,
    pub cors_enabled: bool,
    pub rate_limit: Option<i32>,
}

pub async fn lookup_route(
    pool: &PgPool,
    path: &str,
    method: &str,
) -> anyhow::Result<Option<RouteRecord>> {
    let route = sqlx::query_as::<_, RouteRecord>(
        "SELECT r.id, r.project_id, p.tenant_id, r.path, r.method, r.function_id, r.auth_type, r.cors_enabled, r.rate_limit \
         FROM routes r \
         JOIN projects p ON p.id = r.project_id \
         WHERE r.path = $1 AND r.method = $2 LIMIT 1"
    )
    .bind(path)
    .bind(method)
    .fetch_optional(pool)
    .await?;

    Ok(route)
}
