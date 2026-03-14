-- Add queue_name to jobs and dead_letter_jobs so queue-scoped operations
-- (count, purge, DLQ replay/list) can filter by queue rather than acting
-- on the entire table.
--
-- Nullable because historical rows pre-date this migration; any query that
-- filters on queue_name = $1 will naturally exclude old rows.

ALTER TABLE jobs
    ADD COLUMN IF NOT EXISTS queue_name TEXT;

ALTER TABLE dead_letter_jobs
    ADD COLUMN IF NOT EXISTS queue_name TEXT;

CREATE INDEX IF NOT EXISTS idx_jobs_queue_name
    ON jobs (queue_name, status, run_at)
    WHERE queue_name IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_dlq_queue_name
    ON dead_letter_jobs (queue_name, failed_at)
    WHERE queue_name IS NOT NULL;
