-- Cron job execution history.
--
-- Every time the data-engine fires a cron job it inserts a row here
-- (before dispatching, so the record exists even if the function crashes).
-- The `status` is updated to 'success' or 'failed' after the dispatch completes.

CREATE TABLE IF NOT EXISTS fluxbase_internal.cron_job_runs (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    job_name      TEXT        NOT NULL,
    scheduled_at  TIMESTAMPTZ NOT NULL,  -- the next_run_at value that triggered this run
    started_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at   TIMESTAMPTZ,
    -- 'running' | 'success' | 'failed'
    status        TEXT        NOT NULL DEFAULT 'running',
    error         TEXT,
    request_id    UUID,                  -- linked to execution_records if available
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_cron_job_runs_job_name
    ON fluxbase_internal.cron_job_runs (job_name, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_cron_job_runs_request_id
    ON fluxbase_internal.cron_job_runs (request_id)
    WHERE request_id IS NOT NULL;
