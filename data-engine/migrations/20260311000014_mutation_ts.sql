-- mutation_ts TIMESTAMPTZ
--
-- Records the wall-clock time at which each mutation was written inside its
-- transaction.  DEFAULT now() means it is set automatically — no executor
-- changes needed.
--
-- Why it is separate from created_at:
--   created_at already exists and marks when the row was INSERTed into
--   state_mutations (same value).  mutation_ts is semantically the time the
--   business mutation occurred and is the column used for time-windowed queries.
--   Keeping them separate makes intent explicit and allows them to diverge if
--   backfill or replay scenarios are introduced later.
--
-- Enables time-windowed incident replay and audit without a full table scan:
--
--   SELECT * FROM fluxbase_internal.state_mutations
--   WHERE  mutation_ts BETWEEN $from AND $to
--   ORDER  BY mutation_seq;
--
-- CLI features powered:
--   flux incident replay 2026-03-09T15:00..15:05
--   flux state blame --since 1h
--   flux trace debug --window 5m

ALTER TABLE fluxbase_internal.state_mutations
    ADD COLUMN IF NOT EXISTS mutation_ts TIMESTAMPTZ DEFAULT now();

-- Time-window index for incident replay and audit queries.
-- Supports: WHERE mutation_ts BETWEEN $from AND $to ORDER BY mutation_seq
-- ~8 bytes per row on an already-large table — negligible overhead.
CREATE INDEX IF NOT EXISTS idx_state_mutations_time
    ON fluxbase_internal.state_mutations (mutation_ts);
