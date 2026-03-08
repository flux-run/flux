-- Table metadata registry.
--
-- Stores the authoritative column definitions for every user-created table.
-- Used by:
--   • the policy engine (CLS validation — confirm allowed_columns exist)
--   • the query compiler (reject unknown columns before hitting postgres)
--   • the tables API (schema introspection without touching information_schema)

CREATE TABLE IF NOT EXISTS fluxbase_internal.table_metadata (
    id            UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     UUID NOT NULL,
    project_id    UUID NOT NULL,
    schema_name   TEXT NOT NULL,
    table_name    TEXT NOT NULL,
    -- JSON array of { name, type, not_null, primary_key, unique, default }
    columns       JSONB NOT NULL DEFAULT '[]',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (tenant_id, project_id, schema_name, table_name)
);

CREATE INDEX IF NOT EXISTS idx_table_metadata_lookup
    ON fluxbase_internal.table_metadata (tenant_id, project_id, schema_name);
