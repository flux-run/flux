-- Add per-tenant and per-project indexes to the jobs table.
--
-- The queue fetch query filters on (status, run_at) which is already covered
-- by idx_jobs_pending from 0001_create_jobs.sql.
--
-- Listing jobs for a single tenant or project was doing a full-table scan.
-- These indexes make per-tenant/project job queries O(log n) instead of O(n).

CREATE INDEX IF NOT EXISTS idx_jobs_tenant_id   ON jobs (tenant_id);
CREATE INDEX IF NOT EXISTS idx_jobs_project_id  ON jobs (project_id);
CREATE INDEX IF NOT EXISTS idx_jobs_tenant_status ON jobs (tenant_id, status, run_at);
