-- Migration: 20260313000019_drop_workflow_tables
--
-- Drop the workflow engine tables (created in 007_workflows_and_cron).
--
-- Workflows (multi-step orchestration with branching/looping) are out of scope
-- for the current release. The feature was specced but never implemented.
-- Keeping the tables creates a false impression that workflow execution is live.
--
-- fluxbase_internal.cron_jobs is retained — cron is in scope and will be implemented.
-- fluxbase_internal.workflow_executions must be dropped before workflow_steps,
-- and workflow_steps before workflows (foreign key order).

DROP TABLE IF EXISTS fluxbase_internal.workflow_executions;
DROP TABLE IF EXISTS fluxbase_internal.workflow_steps;
DROP TABLE IF EXISTS fluxbase_internal.workflows;
