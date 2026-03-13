-- Platform users — internal dashboard accounts with RBAC.
--
-- These are NOT end-user accounts for tenant applications.
-- They are operator/admin accounts that log in to the Flux control-plane
-- dashboard.
--
-- Roles:
--   admin    — full read/write on all API routes
--   viewer   — GET requests only (read-only)
--   readonly — alias for viewer

CREATE TABLE IF NOT EXISTS flux.platform_users (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    username      TEXT        UNIQUE NOT NULL,
    email         TEXT        UNIQUE NOT NULL,
    password_hash TEXT        NOT NULL,
    role          TEXT        NOT NULL DEFAULT 'viewer'
                              CHECK (role IN ('admin', 'viewer', 'readonly')),
    tenant_id     UUID,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_platform_users_email
    ON flux.platform_users (email);
