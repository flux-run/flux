use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
    extract::State,
};
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
    // 1. Check direct edge tenant header (Cloudflare Worker optimization)
    let tenant_slug = if let Some(t_slug) = req.headers().get("x-tenant").and_then(|h| h.to_str().ok()) {
        t_slug.to_string()
    } else {
        let host = req
            .headers()
            .get("x-forwarded-host")
            .or_else(|| req.headers().get("host"))
            .and_then(|h| h.to_str().ok())
            .ok_or(StatusCode::BAD_REQUEST)?;

        let parts: Vec<&str> = host.split('.').collect();
        if parts.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }

        let slug = parts[0];

        // Ignore reserved subdomains that point to platform services
        let reserved = ["api", "run", "gateway", "www", "dashboard", "localhost"];
        if reserved.contains(&slug) {
            return Err(StatusCode::NOT_FOUND);
        }
        slug.to_string()
    };

    // 2. Resolve from memory snapshot
    let snapshot_data = state.snapshot.get_data().await;

    if let Some(&tenant_id) = snapshot_data.tenants_by_slug.get(&tenant_slug) {
        req.extensions_mut().insert(ResolvedIdentity {
            tenant_id,
            tenant_slug,
        });
        Ok(next.run(req).await)
    } else {
        tracing::debug!("Tenant not found in memory snapshot: {}", tenant_slug);
        Err(StatusCode::NOT_FOUND)
    }
}
