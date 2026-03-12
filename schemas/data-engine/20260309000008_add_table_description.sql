-- Add user-facing description to table_metadata.
-- The schema handler's fetch_tables query references m.description via
-- COALESCE(m.description, '') — this column was missing from the original DDL.

ALTER TABLE fluxbase_internal.table_metadata
    ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT '';
