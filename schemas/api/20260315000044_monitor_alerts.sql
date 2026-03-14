-- Monitor alerts table.
-- Alerts are threshold-based rules that fire when a metric exceeds
-- a configured threshold.  The dashboard and CLI poll this table for
-- active alerts to surface in `flux monitor alerts` and the dashboard
-- Monitor page.

CREATE TABLE IF NOT EXISTS flux.monitor_alerts (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name        TEXT NOT NULL,
    -- 'error_rate' | 'latency_p95' | 'latency_p99' | 'queue_dlq' | 'queue_failed'
    metric      TEXT NOT NULL,
    -- threshold value (e.g. 0.05 = 5% error rate, 2000 = 2000ms latency)
    threshold   DOUBLE PRECISION NOT NULL,
    -- 'above' | 'below'
    condition   TEXT NOT NULL DEFAULT 'above',
    -- evaluation window in seconds (default: 300 = 5 minutes)
    window_secs INT NOT NULL DEFAULT 300,
    enabled     BOOLEAN NOT NULL DEFAULT true,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    triggered_at TIMESTAMPTZ,
    resolved_at  TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_monitor_alerts_enabled
    ON flux.monitor_alerts (enabled, metric);
