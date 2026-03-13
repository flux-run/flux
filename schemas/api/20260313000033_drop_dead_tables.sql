-- Migration: 20260313000033_drop_dead_tables
--
-- Drop tables that were superseded or never used in production.
--
-- flux.state_mutations
--   Created in 024 as an API-schema mirror of fluxbase_internal.state_mutations.
--   Never populated. All mutation recording is done exclusively by the data-engine
--   which writes to fluxbase_internal.state_mutations. Keeping this table would
--   cause confusion about which is the authoritative source.
--
-- flux.function_logs
--   Legacy per-function log table from early development.
--   Replaced by flux.platform_logs (migration 019) which covers all sources
--   (function, db, queue, event, system) in a single unified table.
--
-- flux.demo_users / flux.demo_requests
--   Landing-page demo signup tables (migration 022).
--   The static marketing site that used these has been removed.
--   Demo signups are no longer collected.

DROP TABLE IF EXISTS flux.state_mutations;
DROP TABLE IF EXISTS flux.function_logs;
DROP TABLE IF EXISTS flux.demo_users;
DROP TABLE IF EXISTS flux.demo_requests;
