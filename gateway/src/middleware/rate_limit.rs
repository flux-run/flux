use dashmap::DashMap;
use std::time::Instant;

struct TokenBucket {
    tokens: f64,
    last_update: Instant,
}

/// Per-key token-bucket rate limiter (thread-safe via DashMap).
///
/// - Refills at `limit_per_sec` tokens/second (burst capacity = limit).
/// - `check(key, limit)` consumes one token; returns `false` → caller should 429.
pub struct RateLimiter {
    buckets: DashMap<String, TokenBucket>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: DashMap::new(),
        }
    }

    /// Returns `true` if the request is within the rate limit, `false` if it should be throttled.
    pub fn check(&self, key: &str, limit_per_sec: u32) -> bool {
        let limit = limit_per_sec as f64;
        let mut bucket = self.buckets.entry(key.to_string()).or_insert(TokenBucket {
            tokens: limit,
            last_update: Instant::now(),
        });

        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_update).as_secs_f64();

        // Refill at limit_per_sec tokens/second, capped at burst capacity (= limit).
        bucket.tokens = (bucket.tokens + elapsed * limit).min(limit);
        bucket.last_update = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Module-level singleton — shared across all requests in the process.
lazy_static::lazy_static! {
    pub static ref LIMITER: RateLimiter = RateLimiter::new();
}

/// Convenience wrapper over the global singleton.
///
/// Returns `true` (allowed) or `false` (throttle → 429).
pub fn check_rate_limit(key: &str, limit_per_sec: u32) -> bool {
    LIMITER.check(key, limit_per_sec)
}

