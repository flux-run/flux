use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use crate::types::scope::Scope;
use crate::types::context::RequestContext;

pub async fn require_scope(
    scope: Scope,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let context = req
        .extensions()
        .get::<RequestContext>()
        .ok_or(StatusCode::UNAUTHORIZED)?;

    match scope {
        Scope::Platform => {
            // Platform scope just needs authentication (which was checked to get the context)
            if context.tenant_id.is_some() || context.project_id.is_some() {
                // strict validation: maybe they shouldn't pass these if not needed, 
                // but usually fine. For now, strictly allow.
            }
        }
        Scope::Tenant => {
            if context.tenant_id.is_none() {
                return Err(StatusCode::FORBIDDEN);
            }
        }
        Scope::Project => {
            if context.tenant_id.is_none() || context.project_id.is_none() {
                return Err(StatusCode::FORBIDDEN);
            }
        }
    }

    Ok(next.run(req).await)
}
