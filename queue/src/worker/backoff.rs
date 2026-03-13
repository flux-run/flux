//! Exponential backoff for job retry scheduling.
//!
//! The formula is `5s × 2^attempts`, giving:
//!
//! | Attempt | Delay |
//! |---------|-------|
//! | 1       | 5 s   |
//! | 2       | 10 s  |
//! | 3       | 20 s  |
//! | 4       | 40 s  |
//! | 5       | 80 s  |
//!
//! The multiplier (5 s) is deliberately conservative to avoid thundering-herd
//! effects when many jobs fail simultaneously (e.g. a downstream service goes down).
//!
//! There is no jitter in the current implementation. If p99 retry storms are
//! observed in production, add ±20 % randomisation to `retry_delay`.
use std::time::Duration;

/// Exponential backoff: 5s * 2^attempts
/// - attempt 1 →  5s
/// - attempt 2 → 10s
/// - attempt 3 → 20s
/// - attempt 4 → 40s
pub fn retry_delay(attempts: u32) -> Duration {
    Duration::from_secs(5 * (1u64 << attempts))
}
