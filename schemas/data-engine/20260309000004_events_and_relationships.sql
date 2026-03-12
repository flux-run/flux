-- Event bus storage.
--
-- After every data mutation (insert/update/delete) the data engine writes a row
-- here.  Queue workers (or a future dedicated event worker) poll/subscribe and
-- fan out to webhooks, workflows, and automation pipelines.
--
-- Events are intentionally append-only; rows are consumed by workers and can be
-- garbage-collected after successful delivery.

CREATE TABLE IF NOT EXISTS fluxbase_internal.events (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL,
    project_id  UUID NOT NULL,
    event_type  TEXT NOT NULL,    -- "users.inserted", "orders.updated", …
    table_name  TEXT NOT NULL,
    -- The primary-key value of the mutated row (as text for portability).
    record_id   TEXT,
    -- "insert" | "update" | "delete"
    operation   TEXT NOT NULL DEFAULT 'insert',
    payload     JSONB NOT NULL DEFAULT '{}',
    -- Set by the event worker once all subscriptions have been dispatched.
    delivered_at TIMESTAMPTZ,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Workers poll: WHERE delivered_at IS NULL ORDER BY created_at FOR UPDATE SKIP LOCKED.
-- Partial index keeps the hot set small as history accumulates.
CREATE INDEX IF NOT EXISTS idx_events_undelivered
    ON fluxbase_internal.events (tenant_id, project_id, created_at)
    WHERE delivered_at IS NULL;

-- ─── Relationships registry ───────────────────────────────────────────────────
--
-- Stores foreign-key-style relationships between user tables.
-- Used initially for documentation and dashboard UI; later powers automatic
-- JOINs in the relational query API (à la PostgREST / Supabase).

CREATE TABLE IF NOT EXISTS fluxbase_internal.relationships (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id       UUID NOT NULL,
    project_id      UUID NOT NULL,
    schema_name     TEXT NOT NULL,

    -- Source side of the relationship.
    from_table      TEXT NOT NULL,
    from_column     TEXT NOT NULL,

    -- Target side.
    to_table        TEXT NOT NULL,
    to_column       TEXT NOT NULL,

    -- "has_one" | "has_many" | "belongs_to" | "many_to_many"
    relationship    TEXT NOT NULL DEFAULT 'has_many',

    -- Human-readable name used in the query API (e.g. "posts", "author").
    alias           TEXT,

    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (tenant_id, project_id, schema_name, from_table, from_column, to_table, to_column)
);

CREATE INDEX IF NOT EXISTS idx_relationships_from
    ON fluxbase_internal.relationships (tenant_id, project_id, schema_name, from_table);
