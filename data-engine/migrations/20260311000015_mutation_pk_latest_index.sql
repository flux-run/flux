-- Covering index for flux state blame — O(log N) latest-mutation lookup.
--
-- Access pattern:
--   SELECT * FROM fluxbase_internal.state_mutations
--   WHERE  tenant_id  = $1
--     AND  project_id = $2
--     AND  table_name = $3
--     AND  record_pk  = $4
--   ORDER  BY mutation_seq DESC
--   LIMIT  1;
--
-- Without this index Postgres scans all mutations for the (tenant, project)
-- pair and sorts.  With it, the index entries are already ordered newest-first
-- so Postgres reads exactly one leaf page and stops.
--
-- Composite key order: tenant → project → table → record_pk → newest-first
-- This means a single index scan satisfies:
--   • point lookup (specific record_pk)        — flux state blame
--   • range scan (all mutations for a table)   — flux state history
--   • DISTINCT ON (record_pk)                  — "who last touched every user?"

CREATE INDEX IF NOT EXISTS idx_state_mutations_pk_latest
    ON fluxbase_internal.state_mutations (
        tenant_id,
        project_id,
        table_name,
        record_pk,
        mutation_seq DESC
    );
