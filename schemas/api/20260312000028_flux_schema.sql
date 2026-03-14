-- Migration: 20260312000028_flux_schema
--
-- Move all Fluxbase-internal tables from the default `public` schema into the
-- dedicated `flux` schema.
--
-- Goal: keep `public` clean for user application data.
-- After this migration the search_path used by all services must include `flux`
-- before `public` so that unqualified table names continue to resolve correctly
-- (set via after_connect on every PgPool — see db/connection.rs).
--
-- User application tables   → public (unchanged)
-- Fluxbase management tables → flux   (moved here)

CREATE SCHEMA IF NOT EXISTS flux;

-- ─── Core auth / identity ──────────────────────────────────────────────────
ALTER TABLE IF EXISTS users             SET SCHEMA flux;
ALTER TABLE IF EXISTS tenants           SET SCHEMA flux;
ALTER TABLE IF EXISTS tenant_members    SET SCHEMA flux;

-- ─── Projects & deployments ───────────────────────────────────────────────
ALTER TABLE IF EXISTS projects          SET SCHEMA flux;
ALTER TABLE IF EXISTS functions         SET SCHEMA flux;
ALTER TABLE IF EXISTS deployments       SET SCHEMA flux;
ALTER TABLE IF EXISTS routes            SET SCHEMA flux;

-- ─── Secrets & API keys ───────────────────────────────────────────────────
ALTER TABLE IF EXISTS secrets           SET SCHEMA flux;
ALTER TABLE IF EXISTS api_keys          SET SCHEMA flux;

-- ─── Observability ────────────────────────────────────────────────────────
ALTER TABLE IF EXISTS trace_requests    SET SCHEMA flux;
ALTER TABLE IF EXISTS trace_signatures  SET SCHEMA flux;
ALTER TABLE IF EXISTS state_mutations   SET SCHEMA flux;
ALTER TABLE IF EXISTS platform_logs     SET SCHEMA flux;
ALTER TABLE IF EXISTS function_logs     SET SCHEMA flux;
ALTER TABLE IF EXISTS audit_logs        SET SCHEMA flux;

-- ─── Platform registry ────────────────────────────────────────────────────
ALTER TABLE IF EXISTS platform_runtimes     SET SCHEMA flux;
ALTER TABLE IF EXISTS platform_services     SET SCHEMA flux;
ALTER TABLE IF EXISTS platform_limits       SET SCHEMA flux;
ALTER TABLE IF EXISTS gateway_metrics       SET SCHEMA flux;
ALTER TABLE IF EXISTS schema_versions       SET SCHEMA flux;
ALTER TABLE IF EXISTS resource_usage        SET SCHEMA flux;

-- ─── Integrations ─────────────────────────────────────────────────────
ALTER TABLE IF EXISTS integrations              SET SCHEMA flux;

-- ─── Demo tables ──────────────────────────────────────────────────────────
ALTER TABLE IF EXISTS demo_requests     SET SCHEMA flux;
ALTER TABLE IF EXISTS demo_users        SET SCHEMA flux;

-- ─── Grant usage on the flux schema to the application role ───────────────
-- Replace `flux_app` with the actual Postgres role if different.
-- This is a no-op if the role does not exist.
DO $$
BEGIN
  IF EXISTS (
    SELECT 1 FROM pg_roles WHERE rolname = current_user
  ) THEN
    EXECUTE format('GRANT USAGE ON SCHEMA flux TO %I', current_user);
    EXECUTE format('GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA flux TO %I', current_user);
    EXECUTE format('ALTER DEFAULT PRIVILEGES IN SCHEMA flux GRANT ALL ON TABLES TO %I', current_user);
  END IF;
END
$$;
