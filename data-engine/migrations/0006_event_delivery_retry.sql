-- Add retry infrastructure to event_deliveries.
--
-- Delivery tracking is per (event × subscription). A delivery row is created
-- *before* dispatch so any process crash leaves it in 'pending', allowing the
-- retry worker to pick it up rather than losing the attempt.
--
-- Retry schedule (exponential, capped at 30 min):
--   attempt 1 → retry_at = dispatched_at + 2s
--   attempt 2 → retry_at = dispatched_at + 4s
--   attempt 3 → retry_at = dispatched_at + 30s
--   attempt 4 → retry_at = dispatched_at + 5min
--   attempt 5 → retry_at = dispatched_at + 30min
--   attempt 6 → status = 'dead_letter', no further retries

ALTER TABLE fluxbase_internal.event_deliveries
    ADD COLUMN IF NOT EXISTS retry_at  TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS next_attempt_at TIMESTAMPTZ;

-- Add max_attempts per subscription (default 5, configurable per sub).
ALTER TABLE fluxbase_internal.event_subscriptions
    ADD COLUMN IF NOT EXISTS max_attempts INT NOT NULL DEFAULT 5;

-- Index for the retry worker: find deliveries ready to re-dispatch.
CREATE INDEX IF NOT EXISTS idx_event_deliveries_retry
    ON fluxbase_internal.event_deliveries (next_attempt_at)
    WHERE status = 'failed' AND next_attempt_at IS NOT NULL;

-- Index for pending deliveries (pre-dispatch reservation).
CREATE INDEX IF NOT EXISTS idx_event_deliveries_pending_created
    ON fluxbase_internal.event_deliveries (created_at)
    WHERE status = 'pending';
