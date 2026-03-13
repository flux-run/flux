//! Per-route token-bucket rate limiter.
//!
//! ## Algorithm — token bucket
//!
//! Each route × client-IP pair gets its own [`Bucket`].  The bucket holds up
//! to `limit_per_sec` tokens and refills continuously at a rate of
//! `limit_per_sec` tokens per second.
//!
//! On every request:
//!   1. Calculate elapsed time since last refill.
//!   2. Add `elapsed × limit_per_sec` tokens, capped at `limit_per_sec` (the
//!      bucket capacity is equal to the per-second limit, so a burst of at
//!      most one second's worth of tokens can accumulate).
//!   3. If `tokens >= 1.0`, consume one and return `true` (allow).
//!   4. Otherwise return `false` (reject — caller returns HTTP 429).
//!
//! ## Process-global state
//!
//! The bucket map is stored in a `DashMap` behind a `OnceLock` so it is
//! shared across all Tokio worker threads without a `Mutex`.  `DashMap`
//! uses shard-level locking internally, keeping contention low even under
//! heavy parallel load.
//!
//! ## Key format
//!
//! Keys are `"{route_uuid}:{client_ip}"` (built by [`key`]).  Using the
//! route UUID (not path) avoids collisions if two routes share a path prefix.
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
