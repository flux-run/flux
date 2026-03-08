// Basic Rate Limiting using DashMap
// In-memory implementation for V1

use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use crate::state::SharedState;

struct TokenBucket {
    tokens: f64,
    last_update: Instant,
}

pub struct RateLimiter {
    buckets: DashMap<String, TokenBucket>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: DashMap::new(),
        }
    }

    pub fn check(&self, key: &str, limit_per_min: i32) -> bool {
        let mut bucket = self.buckets.entry(key.to_string()).or_insert(TokenBucket {
            tokens: limit_per_min as f64,
            last_update: Instant::now(),
        });

        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_update).as_secs_f64();
        let fill_rate = limit_per_min as f64 / 60.0;
        
        bucket.tokens = (bucket.tokens + elapsed * fill_rate).min(limit_per_min as f64);
        bucket.last_update = now;

        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

// Global rate limiter instance for the middleware
lazy_static::lazy_static! {
    static ref LIMITER: RateLimiter = RateLimiter::new();
}

pub fn check_rate_limit(key: &str, limit: i32) -> bool {
    LIMITER.check(key, limit)
}

