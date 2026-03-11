-- Schema Snapshot Cache
--
-- Stores a point-in-time JSON snapshot of (tables + columns + relationships)
-- for each (tenant, project, schema). Used by the Data Engine to serve schema
-- graph requests from a pre-built snapshot instead of querying pg_catalog on
-- every API call.
--
-- Lifecycle:
--   INSERT / UPDATE on schema change (CREATE TABLE, ALTER TABLE, DROP TABLE)
--   READ by GET /db/schema as an optional fast-path (cache-aside)
--   version increments on every write — clients can poll for changes
--
-- This is an optimisation table: the system is always correct without it.
-- The authoritative source of truth is pg_catalog + fluxbase_internal metadata.
--
-- snapshot_json schema:
--   { "tables": [...], "columns": [...], "relationships": [...] }

CREATE TABLE IF NOT EXISTS fluxbase_internal.schema_snapshots (
    id            UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     UUID         NOT NULL,
    project_id    UUID         NOT NULL,
    schema_name   TEXT         NOT NULL,
    snapshot_json JSONB        NOT NULL DEFAULT '{}'::jsonb,
    version       BIGINT       NOT NULL DEFAULT 0,
    updated_at    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    CONSTRAINT uq_schema_snapshot UNIQUE (tenant_id, project_id, schema_name)
);

CREATE INDEX IF NOT EXISTS idx_schema_snapshots_project
    ON fluxbase_internal.schema_snapshots(tenant_id, project_id);

-- Trigger: auto-bump version and updated_at on every update
CREATE OR REPLACE FUNCTION fluxbase_internal.bump_schema_snapshot_version()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    NEW.version    := OLD.version + 1;
    NEW.updated_at := now();
    RETURN NEW;
END;
$$;

CREATE OR REPLACE TRIGGER trg_schema_snapshot_version
    BEFORE UPDATE ON fluxbase_internal.schema_snapshots
    FOR EACH ROW EXECUTE FUNCTION fluxbase_internal.bump_schema_snapshot_version();
