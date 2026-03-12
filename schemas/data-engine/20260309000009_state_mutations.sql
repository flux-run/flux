-- Deterministic execution log: every INSERT / UPDATE / DELETE is captured here
-- inside the same transaction as the user write.
--
-- Purpose:
--   • flux state blame users 42      — who changed this row and when?
--   • flux incident replay 15:00..   — replay all mutations in a time window
--   • flux state inspect users 42    — full history of a single row
--
-- Schema notes:
--   record_pk  — the primary-key value(s) of the affected row, e.g. {"id": 42}
--   before_state — NULL for INSERT; NULL for UPDATE in v1 (pre-read added in v2)
--   after_state  — NULL for DELETE; new row data for INSERT/UPDATE
--   version    — monotonically increasing per (tenant, project, table, record_pk)

CREATE TABLE IF NOT EXISTS fluxbase_internal.state_mutations (
    id           BIGSERIAL    PRIMARY KEY,
    tenant_id    UUID         NOT NULL,
    project_id   UUID         NOT NULL,
    table_name   TEXT         NOT NULL,
    record_pk    JSONB        NOT NULL,
    operation    TEXT         NOT NULL CHECK (operation IN ('insert', 'update', 'delete')),
    before_state JSONB,
    after_state  JSONB,
    version      BIGINT       NOT NULL DEFAULT 1,
    actor_id     TEXT,
    request_id   TEXT,
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- Index 1: row history — used by `flux state inspect users 42`
-- Narrows to (tenant, project, table, record_pk); cheap with pk cardinality.
CREATE INDEX IF NOT EXISTS idx_state_mutations_row
    ON fluxbase_internal.state_mutations(tenant_id, project_id, table_name, record_pk);

-- Index 2: time-window replay — used by `flux incident replay 15:00..15:05`
-- Supports time-range scans across all tables for a tenant/project.
CREATE INDEX IF NOT EXISTS idx_state_mutations_time
    ON fluxbase_internal.state_mutations(tenant_id, project_id, created_at DESC);

-- Index 3: blame (latest-first history) — used by `flux state blame users 42`
-- Allows Postgres to seek directly to a record and walk newest→oldest.
-- Without this, ORDER BY version DESC with a high row count causes a full scan.
CREATE INDEX IF NOT EXISTS idx_state_mutations_pk_version
    ON fluxbase_internal.state_mutations(tenant_id, project_id, table_name, record_pk, version DESC);
