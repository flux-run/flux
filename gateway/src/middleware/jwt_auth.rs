use axum::http::HeaderMap;
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cache::jwks::JwksCache;

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    #[serde(default)]
    pub sub: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(flatten)]
    pub custom: Value,
}

pub async fn verify_jwt(
    headers: &HeaderMap,
    jwks_url: &str,
    audience: Option<&str>,
    issuer: Option<&str>,
    jwks_cache: &JwksCache,
) -> Result<Claims, String> {
    let auth_header = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .ok_or("Missing Authorization header")?;

    if !auth_header.starts_with("Bearer ") {
        return Err("Invalid Authorization header format".to_string());
    }

    let token = &auth_header[7..];

    // Decode header to get kid
    let header = decode_header(token).map_err(|e| format!("Invalid token header: {}", e))?;
    let kid = header.kid.ok_or("Missing 'kid' in token header")?;

    // Load JWKS
    let jwks = jwks_cache
        .get_or_fetch(jwks_url)
        .await
        .ok_or("Failed to fetch JWKS from identity provider")?;

    // Find key
    let jwk = jwks.find(&kid).ok_or("No matching JWK found for kid")?;

    // Validate
    let mut validation = Validation::new(header.alg);
    if let Some(aud) = audience {
        validation.set_audience(&[aud]);
    } else {
        validation.validate_aud = false;
    }
    
    if let Some(iss) = issuer {
        validation.set_issuer(&[iss]);
    }

    let decoding_key = DecodingKey::from_jwk(jwk).map_err(|e| format!("Invalid JWK structural format: {}", e))?;

    let token_data = decode::<Claims>(token, &decoding_key, &validation)
        .map_err(|e| format!("JWT validation failed: {}", e))?;

    Ok(token_data.claims)
}
