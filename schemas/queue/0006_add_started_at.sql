-- Tracks when a worker actually began executing the job (HTTP call to runtime).
-- This is distinct from locked_at which records when the row was claimed by a worker.
-- Timeout is measured from started_at, not locked_at.
ALTER TABLE jobs
    ADD COLUMN IF NOT EXISTS started_at TIMESTAMP;
