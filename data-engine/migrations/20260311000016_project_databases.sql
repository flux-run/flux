-- BYODB: project_databases table
--
-- Stores connection metadata for user-provided (BYODB) PostgreSQL databases.
-- The expected_system_identifier and expected_db_name columns are the safety
-- anchors: when the Data Engine connects to a user pool it MUST verify these
-- match the live cluster, preventing silent data corruption after failover,
-- snapshot restore, or environment misconfiguration.
--
-- expected_system_identifier: value of pg_control_system().system_identifier
--   (unique per physical Postgres cluster; survives logical replica promotion)
-- expected_db_name: value of current_database() at registration time
--   (extra guard against pointing at wrong logical DB on the same host)

CREATE TABLE IF NOT EXISTS fluxbase_internal.project_databases (
    id                          UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id                   UUID    NOT NULL,
    project_id                  UUID    NOT NULL,
    db_name                     TEXT    NOT NULL,   -- logical name used as schema suffix
    connection_url              TEXT    NOT NULL,   -- encrypted at rest by caller
    expected_system_identifier  TEXT,              -- SET on first connect; checked on every reconnect
    expected_db_name            TEXT,              -- PostgreSQL logical database name (current_database())
    created_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                  TIMESTAMPTZ NOT NULL DEFAULT now(),
    CONSTRAINT uq_project_db UNIQUE (tenant_id, project_id, db_name)
);

CREATE INDEX IF NOT EXISTS idx_project_databases_project
    ON fluxbase_internal.project_databases(tenant_id, project_id);
