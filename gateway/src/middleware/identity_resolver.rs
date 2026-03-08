use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
    extract::State,
};
use std::sync::Arc;
use crate::state::SharedState;
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct ResolvedIdentity {
    pub tenant_id: Uuid,
    pub tenant_slug: String,
}

pub async fn resolve_identity(
    State(state): State<SharedState>,
    mut req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    let host = req
        .headers()
        .get("host")
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;

    // Expected format: tenant.api.fluxbase.co
    // For local dev: tenant.localhost:8082
    let parts: Vec<&str> = host.split('.').collect();

    if parts.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let tenant_slug = parts[0].to_string();

    // Resolve from database (ideally cached)
    #[derive(sqlx::FromRow)]
    struct IdentityRow {
        tenant_id: Uuid,
    }

    let result = sqlx::query_as::<_, IdentityRow>(
        r#"
        SELECT id as tenant_id
        FROM tenants
        WHERE slug = $1
        "#,
    )
    .bind(&tenant_slug)
    .fetch_optional(&state.db_pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if let Some(row) = result {
        req.extensions_mut().insert(ResolvedIdentity {
            tenant_id: row.tenant_id,
            tenant_slug,
        });
        Ok(next.run(req).await)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
