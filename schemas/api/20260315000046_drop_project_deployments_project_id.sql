-- Migration: 20260315000046_drop_project_deployments_project_id
--
-- project_deployments.project_id was missed in 20260314000042_drop_tenant_project.
-- With the tenant/project model removed, project_deployments is a global table.

ALTER TABLE flux.project_deployments DROP COLUMN IF EXISTS project_id;
