use std::time::Duration;

/// Exponential backoff: 5s * 2^attempts
/// - attempt 1 →  5s
/// - attempt 2 → 10s
/// - attempt 3 → 20s
/// - attempt 4 → 40s
pub fn retry_delay(attempts: u32) -> Duration {
    Duration::from_secs(5 * (1u64 << attempts))
}
