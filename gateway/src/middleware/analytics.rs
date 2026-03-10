use sqlx::PgPool;
use uuid::Uuid;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;

// ── Channel capacity ──────────────────────────────────────────────────────────
//
// A single background drain worker reads from this channel and writes to
// `gateway_metrics`. If the worker falls behind (e.g., DB slow), new rows are
// silently dropped once the channel is full — bounded memory, no unbounded
// task spawning.
//
// 4 096 rows × ~100 B each ≈ 400 KB; well within budget.
pub const CHANNEL_CAPACITY: usize = 4096;

/// A single sampled row waiting to be written to `gateway_metrics`.
pub struct MetricRow {
    pub route_id:   Uuid,
    pub tenant_id:  Uuid,
    pub status:     u16,
    pub latency_ms: i64,
}

// ── Sampling thresholds (env-overridable) ─────────────────────────────────────
//
// Rules (evaluated in order; first match wins):
//  1. status >= 500                    → always log   (100% of errors)
//  2. latency_ms > SLOW_THRESHOLD_MS  → always log   (100% of slow requests)
//  3. otherwise                        → log 1-in-N via SAMPLE_RATE_SUCCESS (default 10%)
//
// Override via environment:
//   ANALYTICS_SLOW_THRESHOLD_MS=200
//   ANALYTICS_SAMPLE_RATE_SUCCESS=10   (integer 0–100, percent)

fn slow_threshold_ms() -> i64 {
    std::env::var("ANALYTICS_SLOW_THRESHOLD_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(200)
}

fn success_sample_pct() -> u64 {
    // Integer percentage (0–100) avoids floats in atomic round-robin math.
    std::env::var("ANALYTICS_SAMPLE_RATE_SUCCESS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(10)
        .clamp(0, 100)
}

/// Rolling counter — wraps safely at u64::MAX.
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Total metrics dropped because the channel was full.
/// Monotonically increasing; exposed at GET /metrics.
/// A non-zero value means the DB drain worker is falling behind.
pub static DROPPED_METRICS: AtomicU64 = AtomicU64::new(0);

#[inline]
fn should_sample(status: u16, latency_ms: i64) -> bool {
    if status >= 500 { return true; }                           // 100% errors
    if latency_ms > slow_threshold_ms() { return true; }       // 100% slow
    // Deterministic round-robin — exact rate, no randomness needed.
    let n = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pct = success_sample_pct();
    pct > 0 && (n % 100) < pct
}

/// Non-blocking ingestion point called from the hot request path.
///
/// - Runs the sampling check synchronously (cheap; no allocation).
/// - On pass: hands the row to the drain worker via `try_send`.
///   If the channel is full, the row is logged+dropped rather than blocking
///   or spawning an unbounded task.
pub fn log_request(
    tx: &mpsc::Sender<MetricRow>,
    route_id: Uuid,
    tenant_id: Uuid,
    status: u16,
    latency_ms: i64,
) {
    if !should_sample(status, latency_ms) {
        return;
    }
    let row = MetricRow { route_id, tenant_id, status, latency_ms };
    if let Err(_) = tx.try_send(row) {
        // Channel full — drop metric rather than blocking or OOM-ing.
        // Increment the observable counter so operators can alert on this.
        let prev = DROPPED_METRICS.fetch_add(1, Ordering::Relaxed);
        if prev.is_power_of_two() {
            // Log at powers-of-two to avoid spamming on sustained overload.
            tracing::warn!(dropped = prev + 1, "analytics channel full — metrics being dropped (route={route_id}, status={status})");
        }
    }
}

/// Long-running drain worker — spawn exactly **once** at startup.
///
/// Consumes rows from the bounded channel and writes them to `gateway_metrics`
/// sequentially. Exits cleanly when all `Sender` halves are dropped (i.e., on
/// graceful shutdown).
pub async fn drain_worker(mut rx: mpsc::Receiver<MetricRow>, db_pool: PgPool) {
    while let Some(row) = rx.recv().await {
        let metric_id = Uuid::new_v4();
        if let Err(e) = sqlx::query(
            "INSERT INTO gateway_metrics (id, route_id, tenant_id, status, latency_ms) \
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(metric_id)
        .bind(row.route_id)
        .bind(row.tenant_id)
        .bind(i32::from(row.status))
        .bind(row.latency_ms as i32)
        .execute(&db_pool)
        .await
        {
            tracing::error!("gateway analytics write failed: {}", e);
        }
    }
    tracing::info!("analytics drain_worker: channel closed, exiting");
}
