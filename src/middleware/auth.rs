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
    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    
    // 1 Extract Authorization header
    if !auth_header.starts_with("Bearer ") {
        return Err(StatusCode::UNAUTHORIZED);
    }
    
    let token = &auth_header["Bearer ".len()..];

    // 2 Verify Firebase JWT & 3 Extract firebase_uid
    #[cfg(not(test))]
    let (firebase_uid, email) = {
        // The verify call cryptographically checks the JWT signature against Google's public JWKs
        let user: firebase_auth::FirebaseUser = firebase_auth
            .verify(token)
            .map_err(|_| StatusCode::UNAUTHORIZED)?;
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
        role: None,
    };
    req.extensions_mut().insert(context);

    Ok(next.run(req).await)
}
