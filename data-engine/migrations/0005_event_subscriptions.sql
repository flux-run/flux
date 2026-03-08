-- Event subscription registry.
--
-- Maps event patterns to dispatch targets (webhooks, functions, queue jobs).
-- The event worker evaluates each undelivered event against this table and fans
-- out to all matching subscriptions.
--
-- Pattern matching works left-to-right with a single wildcard "*":
--   "users.inserted"  — exact match
--   "users.*"         — any operation on the users table
--   "*"               — all events for this tenant+project

CREATE TABLE IF NOT EXISTS fluxbase_internal.event_subscriptions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL,
    project_id  UUID NOT NULL,

    -- The event type pattern to match (e.g. "users.inserted", "orders.*", "*").
    event_pattern   TEXT NOT NULL,

    -- Dispatch target type: "webhook" | "function" | "queue_job"
    target_type     TEXT NOT NULL,

    -- Target-specific config (see below per target_type):
    --
    --  webhook:
    --    { "url": "https://...", "secret": "...", "headers": {...} }
    --  function:
    --    { "function_id": "<uuid>" }
    --  queue_job:
    --    { "job_type": "...", "queue": "default" }
    target_config   JSONB NOT NULL DEFAULT '{}',

    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (tenant_id, project_id, event_pattern, target_type, target_config)
);

CREATE INDEX IF NOT EXISTS idx_event_subscriptions_tenant_project
    ON fluxbase_internal.event_subscriptions (tenant_id, project_id)
    WHERE enabled = TRUE;

-- Delivery log: one row per (event × subscription) attempt.
-- Persisted so the dashboard can show retry history and the worker can
-- implement back-off without keeping in-process state.
CREATE TABLE IF NOT EXISTS fluxbase_internal.event_deliveries (
    id                  UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    event_id            UUID NOT NULL REFERENCES fluxbase_internal.events(id) ON DELETE CASCADE,
    subscription_id     UUID NOT NULL REFERENCES fluxbase_internal.event_subscriptions(id) ON DELETE CASCADE,
    -- "pending" | "success" | "failed"
    status              TEXT NOT NULL DEFAULT 'pending',
    -- HTTP response status (for webhooks) or null.
    response_status     INT,
    -- Error message on failure.
    error_message       TEXT,
    attempt             INT NOT NULL DEFAULT 1,
    dispatched_at       TIMESTAMPTZ,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_event_deliveries_event
    ON fluxbase_internal.event_deliveries (event_id);

CREATE INDEX IF NOT EXISTS idx_event_deliveries_pending
    ON fluxbase_internal.event_deliveries (created_at)
    WHERE status = 'pending';
