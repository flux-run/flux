-- Drop tenant_id and project_id from queue tables.
--
-- The system is now single-tenant; no per-tenant or per-project scoping
-- is needed in the job queue.

ALTER TABLE jobs            DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE jobs            DROP COLUMN IF EXISTS project_id;

ALTER TABLE dead_letter_jobs DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE dead_letter_jobs DROP COLUMN IF EXISTS project_id;

-- Drop the tenant/project indexes that are now obsolete.
DROP INDEX IF EXISTS idx_jobs_tenant_id;
DROP INDEX IF EXISTS idx_jobs_project_id;
DROP INDEX IF EXISTS idx_jobs_tenant_status;
