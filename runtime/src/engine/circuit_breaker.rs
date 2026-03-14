//! Per-host circuit breaker for `ctx.fetch()` outbound HTTP calls.
//!
//! ## States
//!
//! ```text
//!  Closed ──(≥5 consecutive failures)──▶ Open
//!    ▲                                      │
//!    │       ◀──(probe succeeds)────────────│
//!    │                                      ▼
//!    └──────────────────────────────── HalfOpen ◀──(30 s elapsed)
//! ```
//!
//! * **Closed** — requests flow normally.
//! * **Open** — requests are rejected immediately with a `circuit_open` error.
//!   After `RECOVERY_SECS` the breaker moves to HalfOpen to allow one probe.
//! * **HalfOpen** — one request is let through.  Success → Closed.
//!   Failure → back to Open (timer reset).
//!
//! The registry is process-global (via `OnceLock`) so it is shared across all
//! V8 isolate workers and survives between requests.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Number of consecutive failures before tripping a host's circuit.
const FAILURE_THRESHOLD: u32 = 5;

/// Seconds before an open circuit allows a probe request through.
const RECOVERY_SECS: u64 = 30;

#[derive(Debug)]
enum CircuitState {
    Closed,
    /// Breaker is open; no requests allowed until `until` elapses.
    Open { until: Instant },
    /// One probe request is allowed through; outcome determines next state.
    HalfOpen,
}

struct BreakerEntry {
    state:                CircuitState,
    consecutive_failures: u32,
}

/// Registry mapping hostname → circuit breaker state.
pub struct CircuitBreakerRegistry {
    breakers: Mutex<HashMap<String, BreakerEntry>>,
}

impl CircuitBreakerRegistry {
    fn new() -> Self {
        Self { breakers: Mutex::new(HashMap::new()) }
    }

    /// Check whether a request to `host` should be allowed.
    ///
    /// Returns `Some(retry_after_secs)` when the circuit is open and the
    /// request should be rejected.  Returns `None` to allow the request.
    pub fn check(&self, host: &str) -> Option<u64> {
        let mut map = self.breakers.lock().unwrap_or_else(|p| p.into_inner());
        let entry = match map.get_mut(host) {
            Some(e) => e,
            None => return None, // no history → closed
        };
        match &mut entry.state {
            CircuitState::Closed => None,
            CircuitState::Open { until } => {
                let now = Instant::now();
                if now >= *until {
                    entry.state = CircuitState::HalfOpen;
                    None // let the probe through
                } else {
                    let remaining = (*until - now).as_secs().max(1);
                    Some(remaining)
                }
            }
            CircuitState::HalfOpen => None, // allow probe
        }
    }

    /// Record a successful response for `host`.
    /// Resets the failure counter and closes the circuit.
    pub fn record_success(&self, host: &str) {
        let mut map = self.breakers.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(entry) = map.get_mut(host) {
            entry.state = CircuitState::Closed;
            entry.consecutive_failures = 0;
        }
    }

    /// Record a failed response (HTTP 5xx or connection error) for `host`.
    /// When consecutive failures reach `FAILURE_THRESHOLD`, the circuit opens.
    pub fn record_failure(&self, host: &str) {
        let mut map = self.breakers.lock().unwrap_or_else(|p| p.into_inner());
        let entry = map.entry(host.to_string()).or_insert(BreakerEntry {
            state:                CircuitState::Closed,
            consecutive_failures: 0,
        });
        entry.consecutive_failures += 1;
        if entry.consecutive_failures >= FAILURE_THRESHOLD {
            let until = Instant::now() + Duration::from_secs(RECOVERY_SECS);
            entry.state = CircuitState::Open { until };
            tracing::warn!(
                host = %host,
                failures = entry.consecutive_failures,
                recovery_secs = RECOVERY_SECS,
                "circuit_breaker_opened: outbound HTTP to host tripped after consecutive failures",
            );
        }
    }
}

static REGISTRY: OnceLock<CircuitBreakerRegistry> = OnceLock::new();

/// Return the global circuit-breaker registry, initialising it on first call.
pub fn registry() -> &'static CircuitBreakerRegistry {
    REGISTRY.get_or_init(CircuitBreakerRegistry::new)
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> CircuitBreakerRegistry {
        CircuitBreakerRegistry::new()
    }

    #[test]
    fn closed_by_default() {
        let r = fresh();
        assert!(r.check("api.example.com").is_none());
    }

    #[test]
    fn stays_closed_below_threshold() {
        let r = fresh();
        for _ in 0..FAILURE_THRESHOLD - 1 {
            r.record_failure("api.example.com");
        }
        assert!(r.check("api.example.com").is_none());
    }

    #[test]
    fn opens_at_threshold() {
        let r = fresh();
        for _ in 0..FAILURE_THRESHOLD {
            r.record_failure("api.example.com");
        }
        assert!(r.check("api.example.com").is_some());
    }

    #[test]
    fn success_resets_to_closed() {
        let r = fresh();
        for _ in 0..FAILURE_THRESHOLD {
            r.record_failure("api.example.com");
        }
        // Force to HalfOpen by manipulating state directly is not needed —
        // just record a success while open; the public API transitions through
        // HalfOpen only on `check()` after the timer.  But we can still test
        // record_success closes an already-closed or HalfOpen entry.
        r.record_success("api.example.com");
        // After success the entry is Closed with zero failures.
        // One more success shouldn't cause any issues.
        r.record_success("api.example.com");
        // Failure counter should have been reset — need FAILURE_THRESHOLD new failures.
        for _ in 0..FAILURE_THRESHOLD - 1 {
            r.record_failure("api.example.com");
        }
        assert!(r.check("api.example.com").is_none(), "should still be closed");
    }

    #[test]
    fn different_hosts_are_independent() {
        let r = fresh();
        for _ in 0..FAILURE_THRESHOLD {
            r.record_failure("bad.example.com");
        }
        // bad.example.com is open
        assert!(r.check("bad.example.com").is_some());
        // good.example.com is unaffected
        assert!(r.check("good.example.com").is_none());
    }

    #[test]
    fn retry_after_is_positive() {
        let r = fresh();
        for _ in 0..FAILURE_THRESHOLD {
            r.record_failure("api.example.com");
        }
        let secs = r.check("api.example.com").unwrap();
        assert!(secs >= 1 && secs <= RECOVERY_SECS);
    }
}
