-- Drop tenant_id and project_id columns from all fluxbase_internal tables.
--
-- The system is now a single-binary self-hosted product with one implicit
-- app context. No tenants, no projects.

-- fluxbase_internal.policies (created in 20260309000001_internal_schema.sql)
ALTER TABLE fluxbase_internal.policies          DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.policies          DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal.table_hooks (created in 20260309000001_internal_schema.sql)
ALTER TABLE fluxbase_internal.table_hooks       DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.table_hooks       DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal.table_metadata (created in 20260309000002_table_metadata.sql)
ALTER TABLE fluxbase_internal.table_metadata    DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.table_metadata    DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal.column_metadata (created in 20260309000003_column_metadata_and_hooks.sql)
ALTER TABLE fluxbase_internal.column_metadata   DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.column_metadata   DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal.hooks (created in 20260309000003_column_metadata_and_hooks.sql)
ALTER TABLE fluxbase_internal.hooks             DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.hooks             DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal.events (created in 20260309000004_events_and_relationships.sql)
ALTER TABLE fluxbase_internal.events            DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.events            DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal.relationships (created in 20260309000004_events_and_relationships.sql)
ALTER TABLE fluxbase_internal.relationships     DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.relationships     DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal.event_subscriptions (created in 20260309000005_event_subscriptions.sql)
ALTER TABLE fluxbase_internal.event_subscriptions DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.event_subscriptions DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal.state_mutations (created in 20260309000009_state_mutations.sql)
ALTER TABLE fluxbase_internal.state_mutations   DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.state_mutations   DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal.trace_requests (created in 20260311000013_trace_requests.sql)
ALTER TABLE fluxbase_internal.trace_requests    DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.trace_requests    DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal.project_databases (created in 20260311000016_project_databases.sql)
ALTER TABLE fluxbase_internal.project_databases DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.project_databases DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal.schema_snapshots (created in 20260311000017_schema_snapshots.sql)
ALTER TABLE fluxbase_internal.schema_snapshots  DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE fluxbase_internal.schema_snapshots  DROP COLUMN IF EXISTS project_id;

-- fluxbase_internal workflows/cron tables (created in 20260309000007_workflows_and_cron.sql)
DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'fluxbase_internal' AND table_name = 'workflows') THEN
    ALTER TABLE fluxbase_internal.workflows DROP COLUMN IF EXISTS tenant_id;
    ALTER TABLE fluxbase_internal.workflows DROP COLUMN IF EXISTS project_id;
  END IF;
  IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'fluxbase_internal' AND table_name = 'cron_jobs') THEN
    ALTER TABLE fluxbase_internal.cron_jobs DROP COLUMN IF EXISTS tenant_id;
    ALTER TABLE fluxbase_internal.cron_jobs DROP COLUMN IF EXISTS project_id;
  END IF;
END $$;
