-- Add project-level isolation to the agents table.
--
-- Before this migration agents were globally accessible to all callers,
-- which means any code with API access could read or delete agents belonging
-- to any project.
--
-- Changes:
--   1. Add `project_id` column (nullable for zero-downtime; backfill is the
--      operator's responsibility on existing deployments).
--   2. Add composite index `(project_id, name)` to make per-project lookups
--      fast.
--   3. Drop the old global `UNIQUE (name)` constraint and replace it with
--      `UNIQUE (project_id, name)` so two projects can share an agent name.

ALTER TABLE flux.agents
    ADD COLUMN IF NOT EXISTS project_id UUID REFERENCES projects(id) ON DELETE CASCADE;

-- Replace the global name uniqueness constraint with a per-project one.
-- (If the old constraint does not exist this is a no-op thanks to IF EXISTS.)
ALTER TABLE flux.agents DROP CONSTRAINT IF EXISTS agents_name_key;
ALTER TABLE flux.agents ADD CONSTRAINT agents_project_name_unique
    UNIQUE (project_id, name);

-- Index for per-project queries.
CREATE INDEX IF NOT EXISTS idx_agents_project_id ON flux.agents (project_id);
CREATE INDEX IF NOT EXISTS idx_agents_project_name ON flux.agents (project_id, name);
