-- Maximum seconds a job may remain in 'running' before being considered stuck.
-- A background task will reset stuck jobs to 'pending' so workers can re-pick them.
ALTER TABLE jobs
    ADD COLUMN IF NOT EXISTS max_runtime_seconds INT NOT NULL DEFAULT 300;

CREATE INDEX IF NOT EXISTS idx_jobs_stuck
    ON jobs(status, locked_at)
    WHERE status = 'running';
