-- Intra-request time-travel: link each mutation to the span that caused it.
--
-- With span_id, Fluxbase can reconstruct the exact database state at any
-- point during a request — not just after it finishes.
--
-- Example:
--   request 9624a58d had 4 spans.  Mutations reference span 2 (db.insert)
--   and span 3 (db.update).  Asked "what did state look like during span 2?",
--   Fluxbase returns only the mutations for span 1 and span 2 — everything
--   before and including that span.
--
--   This powers:
--     flux trace debug 9624a58d      — step-through production request
--     flux state at --trace X --span stripe.charge
--     flux trace diff (ordering bugs: mutation happened before vs after a span)
--     Causal graphs: span → mutation → next span
--
-- span_id is optional (NULL for mutations recorded before this migration, or
-- when the caller does not forward x-span-id).  A partial NULL index keeps
-- storage and write overhead near zero for pre-migration rows.

ALTER TABLE fluxbase_internal.state_mutations
    ADD COLUMN IF NOT EXISTS span_id TEXT;

-- Sparse index: only rows that have a span_id (the common case going forward).
-- Supports ORDER BY version within a span and point-in-time state reconstruction.
CREATE INDEX IF NOT EXISTS idx_state_mutations_span
    ON fluxbase_internal.state_mutations (request_id, span_id)
    WHERE span_id IS NOT NULL;
