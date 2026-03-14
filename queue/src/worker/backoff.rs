//! Exponential backoff with jitter for job retry scheduling.
//!
//! The base formula is `5s × 2^attempts`, then ±25% full jitter is applied
//! to prevent thundering-herd storms when many jobs fail simultaneously.
//!
//! | Attempt | Base  | Jitter range        |
//! |---------|-------|---------------------|
//! | 1       | 5 s   | 3.75 s – 6.25 s     |
//! | 2       | 10 s  | 7.5 s  – 12.5 s     |
//! | 3       | 20 s  | 15 s   – 25 s        |
//! | 4       | 40 s  | 30 s   – 50 s        |
//! | 5       | 80 s  | 60 s   – 100 s       |
use rand::Rng;
use std::time::Duration;

/// Exponential backoff with ±25% jitter: base is `5s × 2^attempts`.
pub fn retry_delay(attempts: u32) -> Duration {
    let base_secs = 5.0 * (1u64 << attempts.min(20)) as f64;
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
        // attempts > 20 should be capped (min() on the shift)
        let d = retry_delay(100);
        assert!(d > Duration::from_secs(0));
    }
}
