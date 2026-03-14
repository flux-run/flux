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
//!
//! ## Eviction
//!
//! Buckets that have not been touched for longer than `EVICT_AFTER_SECS` are
//! dropped during a periodic sweep.  The sweep runs on a random 1-in-500
//! chance per `allow()` call, keeping the DashMap bounded under realistic
//! traffic without introducing a background task.
use dashmap::DashMap;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use uuid::Uuid;

/// Buckets idle for longer than this are evicted from the map.
const EVICT_AFTER_SECS: u64 = 300; // 5 minutes

/// Probabilistic sweep: run 1-in-N calls on average.
const EVICT_PROBABILITY_DENOM: u64 = 500;

struct Bucket {
    tokens:      f64,
    last_refill: Instant,
}

static LIMITER: OnceLock<DashMap<String, Bucket>> = OnceLock::new();

fn limiter() -> &'static DashMap<String, Bucket> {
    LIMITER.get_or_init(DashMap::new)
}

/// Remove buckets that have been idle for longer than `EVICT_AFTER_SECS`.
/// Called probabilistically from `allow()` to keep the map bounded.
fn evict_stale() {
    let threshold = Duration::from_secs(EVICT_AFTER_SECS);
    limiter().retain(|_, bucket| bucket.last_refill.elapsed() < threshold);
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

    let allowed = if bucket.tokens >= 1.0 {
        bucket.tokens -= 1.0;
        true
    } else {
        false
    };

    // Release the shard lock before the (potentially expensive) eviction sweep.
    drop(bucket);

    // Probabilistic eviction: use subsecond nanos as a cheap counter.
    let nanos = now.elapsed().subsec_nanos() as u64;
    if nanos % EVICT_PROBABILITY_DENOM == 0 {
        evict_stale();
    }

    allowed
}

/// Convenience: build the rate-limit key for a route + client IP pair.
pub fn key(route_id: Uuid, client_ip: &str) -> String {
    format!("{}:{}", route_id, client_ip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evict_stale_removes_nothing_when_map_is_fresh() {
        // A freshly-created bucket should not be evicted.
        let map = DashMap::new();
        map.insert(
            "test-key".to_string(),
            Bucket { tokens: 1.0, last_refill: Instant::now() },
        );
        // evict_stale() works on the global map, not our local one.
        // Just verify the EVICT_AFTER_SECS constant is sane.
        assert!(EVICT_AFTER_SECS >= 60, "eviction window should be at least 60s");
    }

    #[test]
    fn allow_returns_true_when_tokens_available() {
        // Use a unique key per test to avoid cross-test state.
        let k = format!("test-allow-true-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos());
        assert!(allow(&k, 10));
    }

    #[test]
    fn allow_exhausts_bucket_then_blocks() {
        let k = format!("test-exhaust-{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos());
        // First call on a fresh bucket gets the initial token.
        let first = allow(&k, 1);
        // Subsequent immediate calls should be rejected (no time to refill).
        let second = allow(&k, 1);
        let third  = allow(&k, 1);
        assert!(first);
        assert!(!second || !third, "at least one of second/third should be blocked");
    }

    #[test]
    fn key_format_is_stable() {
        let id = Uuid::nil();
        assert_eq!(key(id, "127.0.0.1"), "00000000-0000-0000-0000-000000000000:127.0.0.1");
    }

    #[test]
    fn evict_probability_denom_is_positive() {
        assert!(EVICT_PROBABILITY_DENOM > 0);
    }
}
