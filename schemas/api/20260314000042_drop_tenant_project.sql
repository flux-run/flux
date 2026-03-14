-- Migration: 20260314000042_drop_tenant_project
--
-- Remove all tenant_id and project_id columns from the flux.* tables.
-- The product is now a single-binary self-hosted system with ONE implicit
-- app context — no tenants, no projects.
--
-- Drop tenants and projects tables first (CASCADE removes FK constraints
-- on all referencing columns automatically).

-- ── Drop tenant/project tables ────────────────────────────────────────────────
DROP TABLE IF EXISTS flux.tenant_members CASCADE;
DROP TABLE IF EXISTS flux.tenants        CASCADE;
DROP TABLE IF EXISTS flux.projects       CASCADE;

-- ── Drop tenant_id / project_id columns ──────────────────────────────────────

ALTER TABLE flux.functions          DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE flux.functions          DROP COLUMN IF EXISTS project_id;

ALTER TABLE flux.secrets            DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE flux.secrets            DROP COLUMN IF EXISTS project_id;

ALTER TABLE flux.api_keys           DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE flux.api_keys           DROP COLUMN IF EXISTS project_id;

ALTER TABLE flux.deployments        DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE flux.deployments        DROP COLUMN IF EXISTS project_id;

ALTER TABLE flux.platform_logs      DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE flux.platform_logs      DROP COLUMN IF EXISTS project_id;

ALTER TABLE flux.platform_users     DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE flux.platform_users     DROP COLUMN IF EXISTS firebase_uid;

ALTER TABLE flux.queue_configs      DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE flux.queue_configs      DROP COLUMN IF EXISTS project_id;

ALTER TABLE flux.environments       DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE flux.environments       DROP COLUMN IF EXISTS project_id;
ALTER TABLE flux.environments       DROP COLUMN IF EXISTS slug;

ALTER TABLE flux.integrations       DROP COLUMN IF EXISTS tenant_id;
ALTER TABLE flux.integrations       DROP COLUMN IF EXISTS project_id;

-- flux.routes was the old routes table (moved to flux schema)
ALTER TABLE flux.routes             DROP COLUMN IF EXISTS project_id;

-- gateway_trace_requests: already had tenant_id dropped in 20260312000030;
-- drop project_id now.
ALTER TABLE flux.gateway_trace_requests DROP COLUMN IF EXISTS project_id;

-- These tables may or may not exist depending on the deployment history.
DO $$ BEGIN
  IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'flux' AND table_name = 'schedules') THEN
    ALTER TABLE flux.schedules DROP COLUMN IF EXISTS tenant_id;
    ALTER TABLE flux.schedules DROP COLUMN IF EXISTS project_id;
  END IF;
  IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'flux' AND table_name = 'gateway_routes') THEN
    ALTER TABLE flux.gateway_routes DROP COLUMN IF EXISTS tenant_id;
    ALTER TABLE flux.gateway_routes DROP COLUMN IF EXISTS project_id;
  END IF;
  IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'flux' AND table_name = 'gateway_middleware') THEN
    ALTER TABLE flux.gateway_middleware DROP COLUMN IF EXISTS tenant_id;
    ALTER TABLE flux.gateway_middleware DROP COLUMN IF EXISTS project_id;
  END IF;
  IF EXISTS (SELECT 1 FROM information_schema.tables WHERE table_schema = 'flux' AND table_name = 'event_subscriptions') THEN
    ALTER TABLE flux.event_subscriptions DROP COLUMN IF EXISTS tenant_id;
    ALTER TABLE flux.event_subscriptions DROP COLUMN IF EXISTS project_id;
  END IF;
END $$;

-- ── Drop demo tables ──────────────────────────────────────────────────────────
DROP TABLE IF EXISTS flux.demo_users    CASCADE;
DROP TABLE IF EXISTS flux.demo_requests CASCADE;
