//! JWT verification with an in-process JWKS key cache.
//!
//! Keys are fetched once per unique JWKS URL and held in memory indefinitely
//! until the key ID (kid) is not found — at which point the cache is
//! invalidated and re-fetched (key rotation).
use axum::http::HeaderMap;
use dashmap::DashMap;
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing::{error, info};

/// Subset of standard + custom JWT claims forwarded to the runtime.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    #[serde(default)]
    pub sub: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
    /// All remaining claims forwarded verbatim to the function.
    #[serde(flatten)]
    pub custom: Value,
}

// ── Key cache ─────────────────────────────────────────────────────────────────

/// Thread-safe in-process JWKS key cache.
///
/// Uses a `DashMap` so reads and writes are concurrent without a global lock.
#[derive(Clone)]
pub struct JwksCache {
    client: Client,
    cache:  Arc<DashMap<String, jsonwebtoken::jwk::JwkSet>>,
}

impl JwksCache {
    pub fn new(client: Client) -> Self {
        Self { client, cache: Arc::new(DashMap::new()) }
    }

    /// Return keys from cache, fetching from `url` on miss.
    pub async fn get_or_fetch(&self, url: &str) -> Option<jsonwebtoken::jwk::JwkSet> {
        if let Some(entry) = self.cache.get(url) {
            return Some(entry.clone());
        }
        info!("Fetching JWKS from {}", url);
        match self.client.get(url).send().await {
            Ok(res) => match res.json::<jsonwebtoken::jwk::JwkSet>().await {
                Ok(jwks) => {
                    self.cache.insert(url.to_string(), jwks.clone());
                    Some(jwks)
                }
                Err(e) => { error!("Failed to parse JWKS from {}: {:?}", url, e); None }
            },
            Err(e) => { error!("Failed to fetch JWKS from {}: {:?}", url, e); None }
        }
    }

    /// Invalidate a cached entry (call when kid lookup fails — key may have rotated).
    pub fn invalidate(&self, url: &str) {
        self.cache.remove(url);
    }
}

// ── JWT verification ──────────────────────────────────────────────────────────

/// Verify the `Authorization: Bearer <token>` header against the given JWKS URL.
///
/// Returns the decoded `Claims` on success, or a human-readable error string.
pub async fn verify(
    headers:    &HeaderMap,
    jwks_url:   &str,
    audience:   Option<&str>,
    issuer:     Option<&str>,
    cache:      &JwksCache,
) -> Result<Claims, String> {
    let bearer = headers
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .filter(|h| h.starts_with("Bearer "))
        .ok_or("Missing or malformed Authorization header")?;

    let token = &bearer[7..];

    let header = decode_header(token)
        .map_err(|e| format!("Invalid token header: {}", e))?;
    let kid = header.kid
        .ok_or("Token is missing 'kid' — cannot select signing key")?;

    let jwks = cache.get_or_fetch(jwks_url).await
        .ok_or("Could not fetch JWKS from identity provider")?;

    // Retry once after cache invalidation in case the key rotated.
    let jwk = match jwks.find(&kid) {
        Some(k) => k.clone(),
        None => {
            cache.invalidate(jwks_url);
            let jwks = cache.get_or_fetch(jwks_url).await
                .ok_or("Could not re-fetch JWKS after key-rotation miss")?;
            jwks.find(&kid)
                .ok_or(format!("No JWK found for kid={}", kid))?
                .clone()
        }
    };

    let mut validation = Validation::new(header.alg);
    match audience {
        Some(aud) => validation.set_audience(&[aud]),
        None      => { validation.validate_aud = false; }
    }
    if let Some(iss) = issuer {
        validation.set_issuer(&[iss]);
    }

    let key = DecodingKey::from_jwk(&jwk)
        .map_err(|e| format!("Invalid JWK key format: {}", e))?;
    let data = decode::<Claims>(token, &key, &validation)
        .map_err(|e| format!("JWT validation failed: {}", e))?;

    Ok(data.claims)
}
