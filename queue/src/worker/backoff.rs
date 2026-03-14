//! Exponential backoff with jitter for job retry scheduling.
//!
//! The base formula is `5s × 2^attempts`, then ±25% full jitter is applied
//! to prevent thundering-herd storms when many jobs fail simultaneously.
//! Attempts are capped at 10 (base ~85 min) to keep retry windows practical.
//!
//! | Attempt | Base    | Jitter range            |
//! |---------|---------|-------------------------|
//! | 1       | 10 s    | 7.5 s  – 12.5 s         |
//! | 2       | 20 s    | 15 s   – 25 s            |
//! | 3       | 40 s    | 30 s   – 50 s            |
//! | 5       | 160 s   | 120 s  – 200 s           |
//! | 8       | 1280 s  | 960 s  – 1600 s          |
//! | 10      | 5120 s  | 3840 s – 6400 s  (~85 min)|
use rand::Rng;
use std::time::Duration;

/// Exponential backoff with ±25% jitter: base is `5s × 2^attempts`.
pub fn retry_delay(attempts: u32) -> Duration {
    // Cap at attempt 10 — base 5s × 2^10 = 5120s (~85 min max before jitter).
    // Capping at 20 would give ~60 days which is impractical for any retry policy.
    let base_secs = 5.0 * (1u64 << attempts.min(10)) as f64;
    // ±25% full jitter — scale factor in [0.75, 1.25)
    let jitter_factor = 1.0 + rand::thread_rng().gen_range(-0.25_f64..0.25_f64);
    let final_secs = (base_secs * jitter_factor).max(1.0); // never less than 1s
    Duration::from_secs_f64(final_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delay_is_at_least_one_second() {
        for attempt in 0..=6 {
            let d = retry_delay(attempt);
            assert!(d >= Duration::from_secs(1), "attempt {} delay was {:?}", attempt, d);
        }
    }

    #[test]
    fn retry_delay_grows_with_attempts() {
        // Base delay doubles per attempt; even with ±25% jitter the median trend holds.
        // Run many samples to verify the median for attempt N+1 > attempt N.
        let samples = 200;
        let avg = |a: u32| -> f64 {
            (0..samples).map(|_| retry_delay(a).as_secs_f64()).sum::<f64>() / samples as f64
        };
        for attempt in 0..5 {
            assert!(
                avg(attempt + 1) > avg(attempt),
                "attempt {} avg should be < attempt {} avg",
                attempt, attempt + 1
            );
        }
    }

    #[test]
    fn retry_delay_within_jitter_bounds() {
        // Verify each sample is within [0.75×base, 1.25×base].
        for attempt in 1..=5 {
            let base = 5.0 * (1u64 << attempt) as f64;
            for _ in 0..50 {
                let d = retry_delay(attempt).as_secs_f64();
                assert!(d >= base * 0.74, "delay {:.2} below lower bound for attempt {}", d, attempt);
                assert!(d <= base * 1.26, "delay {:.2} above upper bound for attempt {}", d, attempt);
            }
        }
    }

    #[test]
    fn retry_delay_does_not_overflow_on_large_attempts() {
        // attempts > 10 are capped — delay is bounded to ~85 min
        let d_10 = retry_delay(10);
        let d_100 = retry_delay(100);
        let d_1000 = retry_delay(1000);
        // All should be equal (same base after cap)
        assert!(d_10 > Duration::from_secs(0));
        // Large-attempt values must not exceed ~6400s (1.25 × 5120s)
        assert!(d_100.as_secs() <= 6_400, "cap failed: {:?}", d_100);
        assert!(d_1000.as_secs() <= 6_400, "cap failed: {:?}", d_1000);
    }
}
