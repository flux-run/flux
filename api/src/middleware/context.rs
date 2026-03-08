use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use uuid::Uuid;
use sqlx::PgPool;
use crate::types::context::RequestContext;

pub async fn resolve_context(
    State(pool): State<PgPool>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if req.method() == axum::http::Method::OPTIONS {
        return Ok(next.run(req).await);
    }
    
    let mut context = req
        .extensions()
        .get::<RequestContext>()
        .cloned()
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // API key auth already populates tenant_id and project_id from the DB.
    // Skip the header-based tenant resolution — API key users aren't in
    // tenant_members so the lookup would always return 403.
    if context.firebase_uid == "api_key" {
        req.extensions_mut().insert(context);
        return Ok(next.run(req).await);
    }

    let tenant_id_str = req
        .headers()
        .get("X-Fluxbase-Tenant")
        .and_then(|h| h.to_str().ok());

    let project_id_str = req
        .headers()
        .get("X-Fluxbase-Project")
        .and_then(|h| h.to_str().ok());

    if let Some(t_id) = tenant_id_str {
        if let Ok(tenant_id) = Uuid::parse_str(t_id) {
            // Verify user belongs to tenant & get role
            let record = sqlx::query_scalar::<_, String>(
                "SELECT role FROM tenant_members WHERE tenant_id = $1 AND user_id = $2"
            )
            .bind(tenant_id)
            .bind(context.user_id)
            .fetch_optional(&pool)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            if let Some(row) = record {
                context.tenant_id = Some(tenant_id);
                context.role = Some(row);
            } else {
                return Err(StatusCode::FORBIDDEN);
            }

            // Project Check (Only valid if tenant exists)
            if let Some(p_id) = project_id_str {
                if let Ok(project_id) = Uuid::parse_str(p_id) {
                    let project_exists = sqlx::query_scalar::<_, Uuid>(
                        "SELECT id FROM projects WHERE id = $1 AND tenant_id = $2"
                    )
                    .bind(project_id)
                    .bind(tenant_id)
                    .fetch_optional(&pool)
                    .await
                    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                    .is_some();

                    if project_exists {
                        context.project_id = Some(project_id);
                    } else {
                         return Err(StatusCode::FORBIDDEN);
                    }
                }
            }
        }
    } else if project_id_str.is_some() {
        // Must provide tenant with project
        return Err(StatusCode::FORBIDDEN);
    }

    req.extensions_mut().insert(context);
    Ok(next.run(req).await)
}
