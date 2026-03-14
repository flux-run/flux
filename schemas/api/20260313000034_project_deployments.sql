-- Add bundle_hash to deployments for change detection.
-- Add project_deployments to track project-level deploy history (rollback support).

-- ── 1. bundle_hash ────────────────────────────────────────────────────────────
-- SHA-256 hex of the bundle bytes. The CLI compares local hashes against this
-- before uploading — unchanged functions are skipped (fast incremental deploys).

ALTER TABLE flux.deployments
    ADD COLUMN IF NOT EXISTS bundle_hash TEXT;

-- ── 2. flux.project_deployments ───────────────────────────────────────────────
-- One row per `flux deploy` run. Groups all function versions deployed together.
-- Used for: `flux deployments list` (history) and `flux deployments rollback <n>`.

CREATE TABLE IF NOT EXISTS flux.project_deployments (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id      UUID        NOT NULL REFERENCES flux.projects(id) ON DELETE CASCADE,
    version         INT         NOT NULL,
    -- Summary of what this deploy changed: { functions: [{ name, version, status }] }
    summary         JSONB       NOT NULL DEFAULT '{}',
    -- Who triggered it: CLI user email or "system" for automated deploys.
    deployed_by     TEXT        NOT NULL DEFAULT 'cli',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Fast lookups: latest deployment per project, rollback by version.
CREATE INDEX IF NOT EXISTS idx_project_deployments_project_version
    ON flux.project_deployments (project_id, version DESC);

-- ── 3. Link individual function deployments back to the project deployment ────
-- Allows rollback to atomically re-activate all function versions from a given
-- project deployment.

ALTER TABLE flux.deployments
    ADD COLUMN IF NOT EXISTS project_deployment_id UUID
        REFERENCES flux.project_deployments(id) ON DELETE SET NULL;
