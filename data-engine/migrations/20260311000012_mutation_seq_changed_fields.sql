-- Deterministic replay ordering + cheap field-level diff support.
--
-- mutation_seq BIGSERIAL
--   A global monotonic counter. Within a single transaction that touches
--   multiple rows across multiple tables, created_at can collide (same
--   clock tick).  mutation_seq provides a strict total order so that:
--
--     SELECT * FROM fluxbase_internal.state_mutations
--     WHERE  request_id = $1
--     ORDER  BY mutation_seq;
--
--   is fully deterministic.  Powers:
--     flux trace debug      — step-through state at each span
--     flux incident replay  — apply mutations in exact write order
--
-- changed_fields TEXT[]
--   For UPDATE operations: array of column names whose value changed.
--   Example: ["plan", "updated_at"]
--
--   Populated by the executor once before-state pre-read is added (v2).
--   NULL for rows written before this migration or for INSERT/DELETE.
--   Enables cheap field-level diff in flux why / flux trace diff without
--   a full key-by-key JSONB comparison.
--
-- schema_name TEXT
--   The Postgres schema the mutation occurred in (e.g. t_acme_auth_main).
--   Completes the full (schema, table, record_pk) identity used by replay.

ALTER TABLE fluxbase_internal.state_mutations
    ADD COLUMN IF NOT EXISTS mutation_seq   BIGSERIAL,
    ADD COLUMN IF NOT EXISTS changed_fields TEXT[],
    ADD COLUMN IF NOT EXISTS schema_name    TEXT;

-- Request-ordered replay within a single request.
-- SELECT ... WHERE request_id = $1 ORDER BY mutation_seq
CREATE INDEX IF NOT EXISTS idx_state_mutations_request_seq
    ON fluxbase_internal.state_mutations (request_id, mutation_seq)
    WHERE request_id IS NOT NULL;

-- Targeted incident replay filtered by table.
-- flux incident replay --request-id X --table users
CREATE INDEX IF NOT EXISTS idx_state_mutations_request_table
    ON fluxbase_internal.state_mutations (request_id, table_name)
    WHERE request_id IS NOT NULL;

-- request_id-only index for joining to trace_requests.
CREATE INDEX IF NOT EXISTS idx_state_mutations_request_id
    ON fluxbase_internal.state_mutations (request_id)
    WHERE request_id IS NOT NULL;
