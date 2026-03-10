use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::Response,
};
use crate::AppState;
use crate::types::context::RequestContext;
use crate::api_keys::{crypto::generate_hash, service::mark_key_used};

pub async fn require_api_key(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !auth_header.starts_with("Bearer ") {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let token = &auth_header["Bearer ".len()..];
    
    // We only process CLI access tokens (flux_...)
    if !token.starts_with("flux_") {
        return Err(StatusCode::UNAUTHORIZED);
    }

    let hash = generate_hash(token);
    
    // Check if key is valid and not revoked, while additionally bumping `last_used_at`
    let api_key = match mark_key_used(&state.pool, &hash).await {
        Ok(k) => k,
        Err(_) => return Err(StatusCode::UNAUTHORIZED),
    };

    // Override the generic RequestContext mapped implicitly behind `resolve_context`
    // Ensure that downstream requests trust this tenant/project natively.
    let ctx = RequestContext {
        user_id: api_key.id,
        firebase_uid: "api_key".to_string(), // Mark this connection differently inherently than standard user sessions
        tenant_id: Some(api_key.tenant_id),
        project_id: Some(api_key.project_id),
        tenant_slug: None,
        project_slug: None,
        role: Some("owner".to_string()),     // API Keys behave as full tenant owners inherently
    };

    req.extensions_mut().insert(ctx);

    Ok(next.run(req).await)
}
