-- Workflow engine tables.
--
-- A workflow is a named sequence of steps triggered by event patterns (the
-- same pattern syntax as event_subscriptions: exact | "{table}.*" | "*").
-- Each step becomes a queue job executed by the existing async worker.
--
-- Execution model:
--   event fires
--     → workflow_executions row created
--       → step 1 enqueued as queue job
--         → on success: step 2 enqueued …
--           → on all steps complete: execution status = 'done'
--
-- Steps are numbered; step_order determines sequencing.

CREATE TABLE IF NOT EXISTS fluxbase_internal.workflows (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID NOT NULL,
    project_id      UUID NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT,
    -- Event pattern that triggers this workflow (same syntax as subscriptions).
    trigger_event   TEXT NOT NULL,
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, name)
);

CREATE TABLE IF NOT EXISTS fluxbase_internal.workflow_steps (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workflow_id     UUID NOT NULL REFERENCES fluxbase_internal.workflows(id) ON DELETE CASCADE,
    step_order      INT NOT NULL,
    name            TEXT NOT NULL,
    -- "function" | "queue_job" | "webhook"
    action_type     TEXT NOT NULL,
    action_config   JSONB NOT NULL DEFAULT '{}',
    -- Optional: only execute this step if the previous step's output matches.
    condition_expr  TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (workflow_id, step_order)
);

-- Runtime execution of a workflow (one row per trigger event).
CREATE TABLE IF NOT EXISTS fluxbase_internal.workflow_executions (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    workflow_id     UUID NOT NULL REFERENCES fluxbase_internal.workflows(id),
    trigger_event_id UUID REFERENCES fluxbase_internal.events(id),
    -- "running" | "done" | "failed" | "cancelled"
    status          TEXT NOT NULL DEFAULT 'running',
    -- Context passed into the first step (the event payload).
    context         JSONB NOT NULL DEFAULT '{}',
    current_step    INT NOT NULL DEFAULT 1,
    error_message   TEXT,
    started_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at     TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_workflow_executions_running
    ON fluxbase_internal.workflow_executions (workflow_id, started_at)
    WHERE status = 'running';

-- ─── Cron scheduler ──────────────────────────────────────────────────────────
--
-- Scheduled triggers expressed as cron expressions (standard 5-field syntax).
-- The cron worker evaluates these every minute and enqueues the appropriate
-- queue job or function call when the schedule fires.

CREATE TABLE IF NOT EXISTS fluxbase_internal.cron_jobs (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID NOT NULL,
    project_id      UUID NOT NULL,
    name            TEXT NOT NULL,
    -- Standard 5-field cron expression: "0 * * * *" = every hour.
    schedule        TEXT NOT NULL,
    -- "function" | "queue_job"
    action_type     TEXT NOT NULL,
    action_config   JSONB NOT NULL DEFAULT '{}',
    enabled         BOOLEAN NOT NULL DEFAULT TRUE,
    last_run_at     TIMESTAMPTZ,
    next_run_at     TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (tenant_id, project_id, name)
);

CREATE INDEX IF NOT EXISTS idx_cron_jobs_due
    ON fluxbase_internal.cron_jobs (next_run_at)
    WHERE enabled = TRUE AND next_run_at IS NOT NULL;
