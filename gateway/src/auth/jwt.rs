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
    ///
    /// # SSRF protection
    /// The URL must use HTTPS and must not target private networks, loopback
    /// addresses, or the cloud instance-metadata endpoint (169.254.169.254).
    pub async fn get_or_fetch(&self, url: &str) -> Option<jsonwebtoken::jwk::JwkSet> {
        if let Err(reason) = validate_jwks_url(url) {
            error!("Rejected JWKS URL '{}': {}", url, reason);
            return None;
        }
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

// ── SSRF protection ───────────────────────────────────────────────────────────

/// Reject JWKS URLs that could be used for server-side request forgery.
///
/// Rules:
/// - Must use `https://` (no plain HTTP).
/// - Host must not be a loopback, link-local, or private-network address.
/// - Blocks the AWS/GCP/Azure instance-metadata IP `169.254.169.254`.
pub fn validate_jwks_url(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url)
        .map_err(|e| format!("invalid URL: {}", e))?;

    if parsed.scheme() != "https" {
        return Err("JWKS URL must use HTTPS".to_string());
    }

    let host = parsed.host_str().unwrap_or("").to_lowercase();

    // Block bare IPs in private / loopback / link-local ranges.
    if let Some(addr) = host.parse::<std::net::IpAddr>().ok() {
        if addr.is_loopback() {
            return Err(format!("JWKS URL targets loopback address: {}", host));
        }
        if is_private_ip(&addr) {
            return Err(format!("JWKS URL targets private/link-local network address: {}", host));
        }
    }

    // Block well-known hostile hostnames regardless of what DNS would resolve them to.
    let blocked_hosts = ["localhost", "metadata.google.internal", "instance-data"];
    for blocked in &blocked_hosts {
        if host == *blocked || host.ends_with(&format!(".{}", blocked)) {
            return Err(format!("JWKS URL targets blocked host: {}", host));
        }
    }

    Ok(())
}

fn is_private_ip(addr: &std::net::IpAddr) -> bool {
    match addr {
        std::net::IpAddr::V4(v4) => {
            let octets = v4.octets();
            // 10.0.0.0/8
            octets[0] == 10
            // 172.16.0.0/12
            || (octets[0] == 172 && (16..=31).contains(&octets[1]))
            // 192.168.0.0/16
            || (octets[0] == 192 && octets[1] == 168)
            // 169.254.0.0/16 (link-local / AWS metadata)
            || (octets[0] == 169 && octets[1] == 254)
        }
        std::net::IpAddr::V6(v6) => {
            // ::1 is handled by is_loopback(); fc00::/7 is unique-local
            let segments = v6.segments();
            (segments[0] & 0xfe00) == 0xfc00
        }
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


// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::validate_jwks_url;

    #[test]
    fn valid_https_url_accepted() {
        assert!(validate_jwks_url("https://accounts.google.com/.well-known/jwks.json").is_ok());
        assert!(validate_jwks_url("https://your-tenant.auth0.com/.well-known/jwks.json").is_ok());
        assert!(validate_jwks_url("https://firebaseappcheck.googleapis.com/v1/jwks").is_ok());
    }

    #[test]
    fn http_url_rejected() {
        let err = validate_jwks_url("http://accounts.google.com/.well-known/jwks.json").unwrap_err();
        assert!(err.contains("HTTPS"), "expected HTTPS error, got: {}", err);
    }

    #[test]
    fn localhost_rejected() {
        assert!(validate_jwks_url("https://localhost/.well-known/jwks.json").is_err());
        assert!(validate_jwks_url("https://localhost:8080/jwks").is_err());
    }

    #[test]
    fn loopback_ip_rejected() {
        assert!(validate_jwks_url("https://127.0.0.1/.well-known/jwks.json").is_err());
        assert!(validate_jwks_url("https://127.0.0.2/jwks").is_err());
    }

    #[test]
    fn aws_metadata_ip_rejected() {
        let err = validate_jwks_url("https://169.254.169.254/latest/meta-data/").unwrap_err();
        assert!(err.contains("private"), "expected private/link-local error, got: {}", err);
    }

    #[test]
    fn private_network_ips_rejected() {
        assert!(validate_jwks_url("https://10.0.0.1/jwks").is_err());
        assert!(validate_jwks_url("https://192.168.1.1/jwks").is_err());
        assert!(validate_jwks_url("https://172.16.0.1/jwks").is_err());
        assert!(validate_jwks_url("https://172.31.255.255/jwks").is_err());
    }

    #[test]
    fn google_internal_hostname_rejected() {
        assert!(validate_jwks_url("https://metadata.google.internal/jwks").is_err());
    }

    #[test]
    fn invalid_url_rejected() {
        assert!(validate_jwks_url("not-a-url").is_err());
        assert!(validate_jwks_url("").is_err());
    }
}
