-- Per-column metadata registry.
--
-- Stores extended column information that Postgres types alone cannot express:
-- file references, computed columns, visibility rules, and SDK hints.
-- Created alongside fluxbase_internal.table_metadata rows when a table is
-- registered via the tables API.

CREATE TABLE IF NOT EXISTS fluxbase_internal.column_metadata (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID NOT NULL,
    project_id   UUID NOT NULL,
    schema_name  TEXT NOT NULL,
    table_name   TEXT NOT NULL,
    column_name  TEXT NOT NULL,

    -- Postgres base type (e.g. "text", "uuid", "timestamptz").
    pg_type      TEXT NOT NULL,
    -- Extended Fluxbase type: "text" | "integer" | "boolean" | "uuid" |
    --   "file" | "computed" | "relation" | ...
    -- "file"     → stored in object storage; pg_type is "text" (stores key/url).
    -- "computed" → derived at query time; no backing column.
    fb_type      TEXT NOT NULL DEFAULT 'default',

    not_null     BOOLEAN NOT NULL DEFAULT false,
    primary_key  BOOLEAN NOT NULL DEFAULT false,
    unique_col   BOOLEAN NOT NULL DEFAULT false,

    -- Default SQL expression (e.g. "gen_random_uuid()", "now()", "'active'").
    default_expr TEXT,

    -- For fb_type = 'file': storage visibility ("public" or "private").
    file_visibility TEXT,
    -- For fb_type = 'file': allowed MIME types (e.g. '["image/png","image/jpeg"]').
    file_accept  JSONB,

    -- For fb_type = 'computed': SQL expression template evaluated at query time.
    -- May reference other columns: "CONCAT(first_name, ' ', last_name)".
    computed_expr TEXT,

    -- Column ordinal position (matches CREATE TABLE order).
    ordinal      INT NOT NULL DEFAULT 0,

    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at   TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (tenant_id, project_id, schema_name, table_name, column_name)
);

CREATE INDEX IF NOT EXISTS idx_column_metadata_table
    ON fluxbase_internal.column_metadata (tenant_id, project_id, schema_name, table_name);

-- ─── Hooks ────────────────────────────────────────────────────────────────────
--
-- Hooks bind a Fluxbase function (runtime function) to a table event.
-- The hooks engine fires them inside the same request lifecycle as the data
-- mutation, giving developers before/after triggers powered by serverless
-- functions.

CREATE TABLE IF NOT EXISTS fluxbase_internal.hooks (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID NOT NULL,
    project_id   UUID NOT NULL,
    table_name   TEXT NOT NULL,

    -- "before_insert" | "after_insert"
    -- "before_update" | "after_update"
    -- "before_delete" | "after_delete"
    event        TEXT NOT NULL,

    -- The deployed runtime function to invoke.
    function_id  UUID NOT NULL,

    -- When false the hook is skipped without error.
    enabled      BOOLEAN NOT NULL DEFAULT true,

    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),

    -- At most one hook per (table, event) per tenant+project.
    UNIQUE (tenant_id, project_id, table_name, event)
);

CREATE INDEX IF NOT EXISTS idx_hooks_lookup
    ON fluxbase_internal.hooks (tenant_id, project_id, table_name, event)
    WHERE enabled = true;
