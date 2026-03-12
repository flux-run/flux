//! Request authentication.
//!
//! The `check()` function is the single entry point.  It dispatches to the
//! appropriate sub-module based on the route's `auth_type` field:
//!
//!   "none"    — public endpoint, no credentials required
//!   "api_key" — `Authorization: Bearer flux_*` or `X-API-Key` header
//!   "jwt"     — Firebase-style JWT validated against a per-route JWKS URL
pub mod api_key;
pub mod jwt;

pub use jwt::JwksCache;

use axum::http::HeaderMap;
use serde_json::Value;
use sqlx::PgPool;
use crate::snapshot::RouteRecord;

/// The result of a successful authentication check.
///
/// Forwarded to the runtime as request-context headers.
#[derive(Debug, Clone)]
pub enum AuthContext {
    /// Route has `auth_type = "none"`.
    Public,
    /// Route has `auth_type = "api_key"` and the key was valid.
    ApiKey,
    /// Route has `auth_type = "jwt"` and the token was valid.
    Jwt {
        user_id: Option<String>,
        claims:  Option<Value>,
    },
    /// LOCAL_MODE — auth skipped, dev identity injected.
    Dev,
}

/// Authenticate the request based on the route's `auth_type`.
///
/// Returns `Err(message)` when credentials are missing or invalid.
pub async fn check(
    pool:       &PgPool,
    jwks_cache: &JwksCache,
    headers:    &HeaderMap,
    route:      &RouteRecord,
) -> Result<AuthContext, String> {
    match route.auth_type.as_str() {
        "none" => Ok(AuthContext::Public),

        "api_key" => {
            let raw_key = headers
                .get("X-API-Key")
                .or_else(|| headers.get("Authorization"))
                .and_then(|h| h.to_str().ok())
                .map(|s| s.trim_start_matches("Bearer ").trim())
                .ok_or("Missing X-API-Key or Authorization header")?;

            match api_key::validate(pool, raw_key).await {
                Ok(true)  => Ok(AuthContext::ApiKey),
                Ok(false) => Err("Invalid or revoked API key".to_string()),
                Err(e)    => Err(format!("API key lookup error: {}", e)),
            }
        }

        "jwt" => {
            let jwks_url = route.jwks_url.as_deref()
                .ok_or("Route requires JWT but has no JWKS URL configured")?;

            let claims = jwt::verify(
                headers,
                jwks_url,
                route.jwt_audience.as_deref(),
                route.jwt_issuer.as_deref(),
                jwks_cache,
            )
            .await?;

            Ok(AuthContext::Jwt {
                user_id: claims.user_id.or(claims.sub),
                claims:  Some(claims.custom),
            })
        }

        other => Err(format!("Unknown auth_type: {}", other)),
    }
}
