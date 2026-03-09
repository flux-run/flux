-- Migration: 20260308000018_schema_versions
-- Track schema versions per project so the SDK can detect changes.

CREATE TABLE IF NOT EXISTS schema_versions (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id     UUID        NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    schema_hash    TEXT        NOT NULL,
    version_number INT         NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(project_id, schema_hash)
);

-- Fast lookup: "what is the latest version for project X?"
CREATE INDEX IF NOT EXISTS idx_schema_versions_project
    ON schema_versions (project_id, version_number DESC);
