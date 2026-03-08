-- fluxbase_internal schema: owned by the data-engine.
-- Stores security policies and lifecycle hook registrations.
-- This schema is never exposed to end users directly.

CREATE SCHEMA IF NOT EXISTS fluxbase_internal;

-- ─── Policies ────────────────────────────────────────────────────────────────
-- Each row grants a role access to a table with optional column and row filters.
--
-- allowed_columns: JSON array of column names. Empty array = all columns allowed.
-- row_condition:   SQL fragment template.  Use $auth.uid, $auth.role,
--                  $auth.tenant_id, $auth.project_id as substitution variables.
--                  Example: "user_id = $auth.uid"
-- operation:       'select' | 'insert' | 'update' | 'delete' | '*'
CREATE TABLE IF NOT EXISTS fluxbase_internal.policies (
    id            UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id     UUID    NOT NULL,
    project_id    UUID    NOT NULL,
    table_name    TEXT    NOT NULL,
    role          TEXT    NOT NULL,
    operation     TEXT    NOT NULL,
    allowed_columns JSONB NOT NULL DEFAULT '[]'::jsonb,
    row_condition TEXT,
    created_at    TIMESTAMP DEFAULT now(),
    CONSTRAINT uq_policy UNIQUE (tenant_id, project_id, table_name, role, operation)
);

CREATE INDEX IF NOT EXISTS idx_policies_lookup
    ON fluxbase_internal.policies(tenant_id, project_id, table_name, role, operation);

-- ─── Table Hooks (Phase 2 — defined now, populated later) ────────────────────
-- Registers serverless functions to run at lifecycle points around table ops.
CREATE TABLE IF NOT EXISTS fluxbase_internal.table_hooks (
    id           UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id    UUID    NOT NULL,
    project_id   UUID    NOT NULL,
    table_name   TEXT    NOT NULL,
    hook_type    TEXT    NOT NULL,  -- before_insert | after_insert | before_read | etc.
    function_id  UUID    NOT NULL,
    enabled      BOOLEAN NOT NULL DEFAULT true,
    created_at   TIMESTAMP DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_hooks_lookup
    ON fluxbase_internal.table_hooks(tenant_id, project_id, table_name, hook_type)
    WHERE enabled = true;
