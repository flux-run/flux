-- Migration: 0012_flux_schema
--
-- Move queue tables from public into the flux schema, consistent with the
-- api/20260312000028_flux_schema.sql migration that moved all other system
-- tables.  Queue tables are Flux internals (not user application data) and
-- belong alongside the rest of the platform tables.
--
-- search_path is set to "flux, public" on every connection (see
-- db/connection.rs in each service), so all unqualified references to jobs,
-- dead_letter_jobs, and job_logs continue to resolve correctly after this
-- migration without any code changes.

CREATE SCHEMA IF NOT EXISTS flux;

ALTER TABLE IF EXISTS jobs              SET SCHEMA flux;
ALTER TABLE IF EXISTS dead_letter_jobs  SET SCHEMA flux;
ALTER TABLE IF EXISTS job_logs          SET SCHEMA flux;
