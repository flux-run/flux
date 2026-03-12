//! Per-route token-bucket rate limiter.
//!
//! Uses a process-global `DashMap` so the same rate-limit counter is shared
//! across all Tokio worker threads without a mutex.
//!
//! `allow(key, limit_per_sec)` tries to consume one token and returns:
//!   true  — request is within the limit, proceed
//!   false — bucket empty, caller should return 429
use dashmap::DashMap;
use std::sync::OnceLock;
use std::time::Instant;
use uuid::Uuid;

struct Bucket {
    tokens:      f64,
    last_refill: Instant,
}

static LIMITER: OnceLock<DashMap<String, Bucket>> = OnceLock::new();

fn limiter() -> &'static DashMap<String, Bucket> {
    LIMITER.get_or_init(DashMap::new)
}

/// Try to consume one token from the bucket identified by `key`.
///
/// `key` is typically `"{route_id}:{client_ip}"` so each route×IP pair
/// gets its own independent counter.
pub fn allow(key: &str, limit_per_sec: u32) -> bool {
    let cap = limit_per_sec as f64;
    let mut bucket = limiter()
        .entry(key.to_string())
        .or_insert_with(|| Bucket { tokens: cap, last_refill: Instant::now() });

    let now     = Instant::now();
    let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
    bucket.tokens      = (bucket.tokens + elapsed * cap).min(cap);
    bucket.last_refill = now;

    if bucket.tokens >= 1.0 {
        bucket.tokens -= 1.0;
        true
    } else {
        false
    }
}

/// Convenience: build the rate-limit key for a route + client IP pair.
pub fn key(route_id: Uuid, client_ip: &str) -> String {
    format!("{}:{}", route_id, client_ip)
}
