use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use uuid::Uuid;
use crate::types::context::RequestContext;
use sqlx::PgPool;

use std::sync::Arc;
use firebase_auth::FirebaseAuth;

pub async fn verify_auth(
    State(pool): State<PgPool>,
    State(firebase_auth): State<Arc<FirebaseAuth>>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if req.method() == axum::http::Method::OPTIONS {
        return Ok(next.run(req).await);
    }

    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    
    // 1 Extract Authorization header
    if !auth_header.starts_with("Bearer ") {
        tracing::error!("Auth header missing 'Bearer ' prefix: {}", auth_header);
        return Err(StatusCode::UNAUTHORIZED);
    }
    
    let token = &auth_header["Bearer ".len()..];

    if token.starts_with("flux_") {
        let hash = crate::api_keys::crypto::generate_hash(token);
        let api_key = match crate::api_keys::service::mark_key_used(&pool, &hash).await {
            Ok(k) => k,
            Err(_) => return Err(StatusCode::UNAUTHORIZED),
        };

        // Resolve the tenant owner so that routes which write to user-FK columns
        // (tenants.owner_id, tenant_members.user_id) receive a valid users.id.
        // Also fetch tenant + project slugs so the data-engine proxy can inject them.
        let tenant_row: Option<(Uuid, String)> = sqlx::query_as(
            "SELECT owner_id, slug FROM tenants WHERE id = $1"
        )
        .bind(api_key.tenant_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten();

        let (owner_id, tenant_slug) = tenant_row
            .map(|(id, slug)| (Some(id), Some(slug)))
            .unwrap_or((None, None));

        let project_slug: Option<String> = sqlx::query_scalar(
            "SELECT slug FROM projects WHERE id = $1"
        )
        .bind(api_key.project_id)
        .fetch_optional(&pool)
        .await
        .ok()
        .flatten();

        let context = RequestContext {
            user_id: owner_id.unwrap_or(api_key.id),
            firebase_uid: "api_key".to_string(),
            tenant_id: Some(api_key.tenant_id),
            project_id: Some(api_key.project_id),
            tenant_slug,
            project_slug,
            role: Some("owner".to_string()),
        };
        req.extensions_mut().insert(context);
        return Ok(next.run(req).await);
    }

    // 2 Verify Firebase JWT & 3 Extract firebase_uid
    #[cfg(not(test))]
    let (firebase_uid, email) = {
        // The verify call cryptographically checks the JWT signature against Google's public JWKs
        let user: firebase_auth::FirebaseUser = firebase_auth
            .verify(token)
            .map_err(|e| {
                tracing::error!("Firebase JWT Verification failed: {:?}", e);
                StatusCode::UNAUTHORIZED
            })?;
        (user.user_id, user.email.unwrap_or_default())
    };

    #[cfg(test)]
    let (firebase_uid, email) = (format!("mock-uid-{}", token), "mock@example.com".to_string());
    
    let user_id = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO users (id, firebase_uid, email, name) VALUES ($1, $2, $3, $4) ON CONFLICT (firebase_uid) DO UPDATE SET email = EXCLUDED.email RETURNING id"
    )
    .bind(Uuid::new_v4())
    .bind(firebase_uid.clone())
    .bind(email)
    .bind("Fluxbase User")
    .fetch_one(&pool)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    
    // 5 Store user_id in request context
    let context = RequestContext {
        user_id,
        firebase_uid,
        tenant_id: None,
        project_id: None,
        tenant_slug: None,
        project_slug: None,
        role: None,
    };
    req.extensions_mut().insert(context);

    Ok(next.run(req).await)
}
