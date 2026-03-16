-- ============================================================
-- Flux — minimal schema
-- web server that records every request
-- ============================================================

CREATE EXTENSION IF NOT EXISTS pgcrypto;
CREATE SCHEMA IF NOT EXISTS flux;

-- ─── Auth: platform users (CLI / dashboard login) ───────────────────────────

CREATE TABLE IF NOT EXISTS flux.platform_users (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    username      TEXT        NOT NULL UNIQUE,
    email         TEXT        NOT NULL UNIQUE,
    password_hash TEXT        NOT NULL,
    role          TEXT        NOT NULL DEFAULT 'admin'
                              CHECK (role IN ('admin', 'viewer', 'readonly')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_platform_users_email ON flux.platform_users (email);

-- ─── Auth: API keys (CLI/service bearer keys) ───────────────────────────────

CREATE TABLE IF NOT EXISTS flux.api_keys (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name         TEXT        NOT NULL,
    key_hash     TEXT        NOT NULL UNIQUE,
    key_prefix   TEXT        NOT NULL,
    role         TEXT        NOT NULL DEFAULT 'admin'
                              CHECK (role IN ('admin', 'viewer', 'readonly')),
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    revoked_at   TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_api_keys_active
    ON flux.api_keys (created_at DESC)
    WHERE revoked_at IS NULL;

-- ─── Auth: runtime service token (internal runtime → server auth) ──────────
-- Current server checks X-Service-Token against INTERNAL_SERVICE_TOKEN.
-- Keep an optional DB record for visibility/rotation metadata.

CREATE TABLE IF NOT EXISTS flux.service_tokens (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    service_name TEXT        NOT NULL,
    token_hash   TEXT        NOT NULL UNIQUE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    revoked_at   TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_service_tokens_active
    ON flux.service_tokens (service_name, created_at DESC)
    WHERE revoked_at IS NULL;

-- ─── Projects ────────────────────────────────────────────────
-- one row per user project streaming to this server

CREATE TABLE IF NOT EXISTS flux.projects (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT        NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- ─── Executions ──────────────────────────────────────────────
-- one row per incoming HTTP request

CREATE TABLE IF NOT EXISTS flux.executions (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  UUID        NOT NULL REFERENCES flux.projects(id),
    method      TEXT        NOT NULL,
    path        TEXT        NOT NULL,
    status      TEXT        NOT NULL DEFAULT 'running'
                            CHECK (status IN ('running','ok','error','timeout')),
    status_code INT,
    request     JSONB,      -- headers + body
    response    JSONB,      -- headers + body
    error       TEXT,
    code_sha    TEXT,
    started_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    duration_ms INT,
    -- webhook causation
    parent_id   UUID        REFERENCES flux.executions(id)
);

CREATE INDEX ON flux.executions (project_id, started_at DESC);
CREATE INDEX ON flux.executions (status, started_at DESC);
CREATE INDEX ON flux.executions (parent_id) WHERE parent_id IS NOT NULL;

-- ─── Checkpoints ─────────────────────────────────────────────
-- one row per IO boundary crossing (fetch or db)
-- call_index is the replay key — match by index not by url

CREATE TABLE IF NOT EXISTS flux.checkpoints (
    id           UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    execution_id UUID    NOT NULL REFERENCES flux.executions(id) ON DELETE CASCADE,
    call_index   INT     NOT NULL,
    boundary     TEXT    NOT NULL CHECK (boundary IN ('http','db')),
    request      BYTEA   NOT NULL,
    response     BYTEA   NOT NULL,
    duration_ms  INT     NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (execution_id, call_index)
);

CREATE INDEX ON flux.checkpoints (execution_id, call_index);

-- ─── Mutations ───────────────────────────────────────────────
-- one row per db write, recorded inside same transaction

CREATE TABLE IF NOT EXISTS flux.mutations (
    id           UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    execution_id UUID    NOT NULL REFERENCES flux.executions(id) ON DELETE CASCADE,
    call_index   INT     NOT NULL,
    table_name   TEXT    NOT NULL,
    operation    TEXT    NOT NULL CHECK (operation IN ('insert','update','delete')),
    row_id       TEXT,
    before_state JSONB,
    after_state  JSONB,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX ON flux.mutations (execution_id);
CREATE INDEX ON flux.mutations (table_name, row_id, created_at DESC);

-- ─── Logs ────────────────────────────────────────────────────
-- console.log lines from inside the isolate

CREATE TABLE IF NOT EXISTS flux.logs (
    id           UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    execution_id UUID    NOT NULL REFERENCES flux.executions(id) ON DELETE CASCADE,
    level        TEXT    NOT NULL DEFAULT 'info',
    message      TEXT    NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX ON flux.logs (execution_id, created_at ASC);

-- ─── Deployments ─────────────────────────────────────────────
-- content-addressed code versions
-- sha256 of bundle is the version identifier

CREATE TABLE IF NOT EXISTS flux.deployments (
    sha          TEXT        PRIMARY KEY,
    project_id   UUID        NOT NULL REFERENCES flux.projects(id),
    artifact_bytes BYTEA     NOT NULL,
    artifact_encoding TEXT   NOT NULL DEFAULT 'raw',
    deployed_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX ON flux.deployments (project_id, deployed_at DESC);

-- ─── Notify ──────────────────────────────────────────────────
-- flux tail listens on this channel

CREATE OR REPLACE FUNCTION flux.notify_execution()
RETURNS trigger AS $$
BEGIN
    PERFORM pg_notify('flux_executions', row_to_json(NEW)::text);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_execution_notify ON flux.executions;
CREATE TRIGGER trg_execution_notify
    AFTER INSERT OR UPDATE ON flux.executions
    FOR EACH ROW EXECUTE FUNCTION flux.notify_execution();
