-- Add a role column to flux.api_keys so DB-stored API keys are subject to
-- the same RBAC rules as JWT sessions.
--
-- Existing keys default to 'admin' (backward-compatible — all previously
-- created keys were created by an admin and should retain full access).
-- New keys can be created with role = 'viewer' for read-only CLI access.

ALTER TABLE flux.api_keys
    ADD COLUMN IF NOT EXISTS role TEXT NOT NULL DEFAULT 'admin';
