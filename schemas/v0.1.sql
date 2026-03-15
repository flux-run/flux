-- ============================================================
-- Flux v0.1 — Consolidated baseline schema
-- ============================================================
--
-- This single file represents the complete, final database
-- state reached after all incremental migrations in:
--   schemas/api/            (46 files)
--   schemas/data-engine/    (21 files)
--   schemas/queue/          (12 files)
--
-- Usage
-- -----
--   Fresh install:    psql $DATABASE_URL -f schemas/v0.1.sql
--   Existing install: continue running the migration runner as
--                     before — this file is a reference / clean
--                     re-install baseline only.
--
-- Design decisions encoded here
-- ------------------------------
--   • All Flux system tables live in the `flux` schema.
--   • All Data Engine introspection tables live in `flux_internal`.
--   • `public` is reserved exclusively for user application data.
--   • search_path on every connection is set to "flux, public" so
--     unqualified names resolve to flux.* first (see db/connection.rs).
--   • Tenant / project columns have been removed — single-binary,
--     single-app model.
--   • Bundle bytes are NOT stored in Postgres; bundles live on the
--     filesystem under FLUX_FUNCTIONS_DIR.
--
-- Omitted (superseded / unused tables)
-- -------------------------------------
--   flux.users            — Firebase-auth table; replaced by platform_users
--   flux.resource_usage   — Has broken NOT NULL tenant_id; no code references it
--   flux.audit_logs       — Orphaned; unused in current service code
--   flux.platform_limits  — Tenant-keyed PK; meaningless without tenants
-- ============================================================

-- ─── Extensions ──────────────────────────────────────────────────────────────

CREATE EXTENSION IF NOT EXISTS pgcrypto;

-- ─── Schemas ─────────────────────────────────────────────────────────────────

CREATE SCHEMA IF NOT EXISTS flux;
CREATE SCHEMA IF NOT EXISTS flux_internal;

-- ─── Schema grants ───────────────────────────────────────────────────────────
-- Grants the current connecting role full access to both schemas.
-- This is idempotent and a no-op if the role has no separate identity.

DO $$
BEGIN
  IF EXISTS (SELECT 1 FROM pg_roles WHERE rolname = current_user) THEN
    EXECUTE format('GRANT USAGE ON SCHEMA flux TO %I', current_user);
    EXECUTE format('GRANT USAGE ON SCHEMA flux_internal TO %I', current_user);
    EXECUTE format('GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA flux TO %I', current_user);
    EXECUTE format('GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA flux_internal TO %I', current_user);
    EXECUTE format('ALTER DEFAULT PRIVILEGES IN SCHEMA flux GRANT ALL ON TABLES TO %I', current_user);
    EXECUTE format('ALTER DEFAULT PRIVILEGES IN SCHEMA flux_internal GRANT ALL ON TABLES TO %I', current_user);
  END IF;
END
$$;


-- ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
-- flux.*  —  Platform / Control-plane tables
-- ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

-- ─── Auth ────────────────────────────────────────────────────────────────────

-- Dashboard operator accounts.  NOT end-user application accounts.
-- Roles: admin (full read/write) | viewer / readonly (GET only).
CREATE TABLE IF NOT EXISTS flux.platform_users (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    username      TEXT        UNIQUE NOT NULL,
    email         TEXT        UNIQUE NOT NULL,
    password_hash TEXT        NOT NULL,
    role          TEXT        NOT NULL DEFAULT 'viewer'
                              CHECK (role IN ('admin', 'viewer', 'readonly')),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_platform_users_email
    ON flux.platform_users (email);


-- ─── Functions & Deployments ─────────────────────────────────────────────────

-- Function registry — one row per named function.
CREATE TABLE IF NOT EXISTS flux.functions (
    id            UUID  PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT  NOT NULL UNIQUE,
    runtime       TEXT  NOT NULL DEFAULT 'deno',
    description   TEXT,
    input_schema  JSONB,
    output_schema JSONB,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Versioned deployment records.  is_active marks the currently live version.
-- bundle_code / bundle_url are NOT stored here — bundles live on the filesystem
-- under FLUX_FUNCTIONS_DIR (see docs: fs-bundles).
CREATE TABLE IF NOT EXISTS flux.deployments (
    id                    UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    function_id           UUID    NOT NULL REFERENCES flux.functions(id) ON DELETE CASCADE,
    storage_key           TEXT    NOT NULL,
    version               INT     NOT NULL DEFAULT 1,
    is_active             BOOLEAN NOT NULL DEFAULT FALSE,
    status                TEXT    NOT NULL DEFAULT 'ready',
    bundle_hash           TEXT,
    -- populated after project_deployments is created; FK added below
    project_deployment_id UUID,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_deployments_function
    ON flux.deployments (function_id, version DESC);

-- One record per `flux deploy` run.  Groups all function versions deployed
-- together.  Used for `flux deployments list` and `flux deployments rollback`.
CREATE TABLE IF NOT EXISTS flux.project_deployments (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    version     INT         NOT NULL,
    summary     JSONB       NOT NULL DEFAULT '{}',
    deployed_by TEXT        NOT NULL DEFAULT 'cli',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_project_deployments_version
    ON flux.project_deployments (version DESC);

-- Add FK now that project_deployments exists.  NOT VALID skips back-fill scan.
-- ADD CONSTRAINT IF NOT EXISTS was added in PG 17; use DO block for compatibility.
DO $$
BEGIN
  ALTER TABLE flux.deployments
    ADD CONSTRAINT fk_deployments_project_deployment
    FOREIGN KEY (project_deployment_id)
    REFERENCES flux.project_deployments(id) ON DELETE SET NULL
    NOT VALID;
EXCEPTION
  WHEN duplicate_object THEN NULL;
END;
$$;


-- ─── Routing ─────────────────────────────────────────────────────────────────

-- Route table — the gateway's authoritative dispatch config.
-- The gateway keeps an in-memory snapshot and refreshes it via
-- LISTEN/NOTIFY on `route_changes` (trigger defined at the end of this file).
CREATE TABLE IF NOT EXISTS flux.routes (
    id                    UUID    PRIMARY KEY DEFAULT gen_random_uuid(),
    path                  TEXT    NOT NULL,
    method                TEXT    NOT NULL DEFAULT 'POST',
    function_name         TEXT    NOT NULL,
    middleware            JSONB   NOT NULL DEFAULT '[]',
    rate_limit_per_minute INT,
    project_deployment_id UUID    REFERENCES flux.project_deployments(id) ON DELETE SET NULL,
    is_active             BOOLEAN NOT NULL DEFAULT TRUE,
    auth_type             TEXT    NOT NULL DEFAULT 'none',
    cors_enabled          BOOLEAN NOT NULL DEFAULT FALSE,
    jwks_url              TEXT,
    jwt_audience          TEXT,
    jwt_issuer            TEXT,
    json_schema           JSONB,
    cors_origins          TEXT[],
    cors_headers          TEXT[],
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_routes_active
    ON flux.routes (is_active)
    WHERE is_active = TRUE;

-- One active route per (method, path).
CREATE UNIQUE INDEX IF NOT EXISTS idx_routes_method_path_active
    ON flux.routes (method, path)
    WHERE is_active = TRUE;


-- ─── API Keys ────────────────────────────────────────────────────────────────

-- SHA-256 hashed API keys.  key_prefix is stored for display (e.g. "flux_…").
-- role mirrors platform_users RBAC: admin | viewer.
CREATE TABLE IF NOT EXISTS flux.api_keys (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name         TEXT        NOT NULL UNIQUE,
    key_hash     TEXT        NOT NULL,
    key_prefix   TEXT        NOT NULL,
    role         TEXT        NOT NULL DEFAULT 'admin',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    revoked_at   TIMESTAMPTZ
);


-- ─── Secrets ─────────────────────────────────────────────────────────────────

-- AES-256-GCM encrypted secrets.  Raw plaintext is NEVER stored.
-- encrypted_value holds the ciphertext; value is a migration-era column
-- that should be NULL / ignored in new code.
CREATE TABLE IF NOT EXISTS flux.secrets (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    key             TEXT        NOT NULL UNIQUE,
    encrypted_value TEXT        NOT NULL,
    value           TEXT,
    scope           TEXT        NOT NULL DEFAULT 'project',
    version         INT         NOT NULL DEFAULT 1,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);


-- ─── Observability ───────────────────────────────────────────────────────────

-- Unified log stream covering: function | db | queue | event | system.
CREATE TABLE IF NOT EXISTS flux.platform_logs (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    source          TEXT        NOT NULL DEFAULT 'function',
    resource_id     TEXT        NOT NULL DEFAULT '',
    level           TEXT        NOT NULL DEFAULT 'info',
    message         TEXT        NOT NULL,
    request_id      TEXT,
    metadata        JSONB,
    timestamp       TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    -- structured trace / span fields
    span_type       TEXT,
    parent_span_id  UUID,
    span_id         UUID,
    code_sha        TEXT,
    code_location   TEXT,
    checkpoint_type TEXT,
    execution_state JSONB
);

CREATE INDEX IF NOT EXISTS idx_platform_logs_source_resource
    ON flux.platform_logs (source, resource_id, timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_platform_logs_ts
    ON flux.platform_logs (timestamp DESC);

CREATE INDEX IF NOT EXISTS idx_platform_logs_timestamp_asc
    ON flux.platform_logs (timestamp ASC);

CREATE INDEX IF NOT EXISTS idx_platform_logs_parent_span_id
    ON flux.platform_logs (parent_span_id)
    WHERE parent_span_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_platform_logs_code_sha
    ON flux.platform_logs (code_sha)
    WHERE code_sha IS NOT NULL;

-- Complete request envelope recorded by the gateway at the start of every call.
-- Golden source for `flux incident replay <id>`.
CREATE TABLE IF NOT EXISTS flux.gateway_trace_requests (
    request_id      TEXT        PRIMARY KEY,
    method          TEXT        NOT NULL,
    path            TEXT        NOT NULL,
    headers         JSONB,
    query_params    JSONB,
    body            JSONB,
    response_status INT,
    response_body   JSONB,
    duration_ms     INT,
    function_name   TEXT        NOT NULL DEFAULT '',
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_gateway_trace_function
    ON flux.gateway_trace_requests (function_name, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_gateway_trace_created_at
    ON flux.gateway_trace_requests (created_at ASC);

-- Behavioral fingerprints used by `flux bug bisect` for regression detection.
CREATE TABLE IF NOT EXISTS flux.trace_signatures (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    request_id     TEXT        NOT NULL,
    function_id    UUID        NOT NULL,
    code_sha       TEXT        NOT NULL,
    signature_hash TEXT        NOT NULL,
    status_code    INT,
    latency_ms     INT,
    error_type     TEXT,
    error_message  TEXT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_trace_signatures_function_ts
    ON flux.trace_signatures (function_id, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_trace_signatures_code_sha
    ON flux.trace_signatures (code_sha, function_id);

CREATE INDEX IF NOT EXISTS idx_trace_signatures_signature_hash
    ON flux.trace_signatures (signature_hash, function_id);

CREATE INDEX IF NOT EXISTS idx_trace_signatures_request_id
    ON flux.trace_signatures (request_id);

-- Schema version tracking — detects when `flux db push` changes the schema.
CREATE TABLE IF NOT EXISTS flux.schema_versions (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    schema_hash    TEXT        NOT NULL,
    version_number INT         NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (schema_hash)
);

CREATE INDEX IF NOT EXISTS idx_schema_versions_number
    ON flux.schema_versions (version_number DESC);


-- ─── Platform registry ────────────────────────────────────────────────────────
-- Read by the dashboard / status API; updated manually or via deploy pipeline.

CREATE TABLE IF NOT EXISTS flux.platform_runtimes (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT        NOT NULL UNIQUE,
    engine     TEXT        NOT NULL,
    status     TEXT        NOT NULL DEFAULT 'disabled',
    version    TEXT        NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO flux.platform_runtimes (name, engine, status, version) VALUES
    ('deno',   'rust_deno_engine', 'active',   '1.0.0'),
    ('nodejs', 'node_runtime',     'disabled', '20.x'),
    ('python', 'python_runtime',   'disabled', '3.11')
ON CONFLICT (name) DO NOTHING;

CREATE TABLE IF NOT EXISTS flux.platform_services (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT        NOT NULL UNIQUE,
    status     TEXT        NOT NULL DEFAULT 'disabled',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

INSERT INTO flux.platform_services (name, status) VALUES
    ('serverless', 'active'),
    ('events',     'active'),
    ('database',   'disabled'),
    ('queue',      'active'),
    ('storage',    'disabled')
ON CONFLICT (name) DO NOTHING;

-- Gateway request / response metrics (populated by the gateway on every call).
CREATE TABLE IF NOT EXISTS flux.gateway_metrics (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    route_id   UUID,
    status     INT,
    latency_ms INT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_gateway_metrics_route
    ON flux.gateway_metrics (route_id, created_at DESC);


-- ─── Integrations ────────────────────────────────────────────────────────────

-- External OAuth connections (Composio-backed).
CREATE TABLE IF NOT EXISTS flux.integrations (
    id                     UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    provider               VARCHAR(100) NOT NULL UNIQUE,
    account_label          VARCHAR(255),
    composio_connection_id VARCHAR(255),
    status                 VARCHAR(50)  NOT NULL DEFAULT 'pending',
    metadata               JSONB        NOT NULL DEFAULT '{}',
    connected_at           TIMESTAMPTZ,
    created_at             TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);


-- ─── Queue configuration ─────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS flux.queue_configs (
    id                    UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name                  TEXT        NOT NULL UNIQUE,
    description           TEXT,
    max_attempts          INT         NOT NULL DEFAULT 5,
    visibility_timeout_ms BIGINT      NOT NULL DEFAULT 30000,
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Queue bindings: links a named queue to the function that consumes it.
-- The queue worker refreshes this map via LISTEN/NOTIFY on `queue_bindings_changed`.
CREATE TABLE IF NOT EXISTS flux.queue_bindings (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    queue_name  TEXT        NOT NULL,
    function_id UUID        NOT NULL REFERENCES flux.functions(id) ON DELETE CASCADE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (queue_name, function_id)
);

CREATE INDEX IF NOT EXISTS idx_queue_bindings_queue_name
    ON flux.queue_bindings (queue_name);


-- ─── Environments ────────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS flux.environments (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name       TEXT        NOT NULL UNIQUE,
    is_default BOOLEAN     NOT NULL DEFAULT FALSE,
    config     JSONB       NOT NULL DEFAULT '{}',
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);


-- ─── Monitor / alerts ────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS flux.monitor_alerts (
    id           UUID             PRIMARY KEY DEFAULT gen_random_uuid(),
    name         TEXT             NOT NULL,
    -- 'error_rate' | 'latency_p95' | 'latency_p99' | 'queue_dlq' | 'queue_failed'
    metric       TEXT             NOT NULL,
    threshold    DOUBLE PRECISION NOT NULL,
    condition    TEXT             NOT NULL DEFAULT 'above',
    window_secs  INT              NOT NULL DEFAULT 300,
    enabled      BOOLEAN          NOT NULL DEFAULT true,
    created_at   TIMESTAMPTZ      NOT NULL DEFAULT now(),
    triggered_at TIMESTAMPTZ,
    resolved_at  TIMESTAMPTZ
);

CREATE INDEX IF NOT EXISTS idx_monitor_alerts_enabled
    ON flux.monitor_alerts (enabled, metric);


-- ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
-- flux.*  —  Queue tables
-- (moved from public → flux by queue migration 0012)
-- ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

CREATE TABLE IF NOT EXISTS flux.jobs (
    id                  UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    function_id         UUID        NOT NULL,
    payload             JSONB,
    status              TEXT        NOT NULL DEFAULT 'pending',
    attempts            INT         NOT NULL DEFAULT 0,
    max_attempts        INT         NOT NULL DEFAULT 5,
    run_at              TIMESTAMPTZ NOT NULL DEFAULT now(),
    locked_at           TIMESTAMPTZ,
    max_runtime_seconds INT         NOT NULL DEFAULT 300,
    started_at          TIMESTAMPTZ,
    request_id          UUID,
    idempotency_key     TEXT,
    queue_name          TEXT,
    created_at          TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_jobs_pending
    ON flux.jobs (status, run_at);

CREATE INDEX IF NOT EXISTS idx_jobs_stuck
    ON flux.jobs (status, locked_at)
    WHERE status = 'running';

CREATE INDEX IF NOT EXISTS idx_jobs_request_id
    ON flux.jobs (request_id)
    WHERE request_id IS NOT NULL;

CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_idempotency_key
    ON flux.jobs (idempotency_key)
    WHERE idempotency_key IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_jobs_queue_name
    ON flux.jobs (queue_name, status, run_at)
    WHERE queue_name IS NOT NULL;

CREATE TABLE IF NOT EXISTS flux.job_logs (
    id         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    job_id     UUID        REFERENCES flux.jobs(id) ON DELETE CASCADE,
    message    TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS flux.dead_letter_jobs (
    id          UUID        PRIMARY KEY,
    function_id UUID,
    payload     JSONB,
    error       TEXT,
    failed_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    queue_name  TEXT
);

CREATE INDEX IF NOT EXISTS idx_dlq_queue_name
    ON flux.dead_letter_jobs (queue_name, failed_at)
    WHERE queue_name IS NOT NULL;


-- ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
-- flux_internal.*  —  Data-engine tables
-- ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

-- ─── Schema introspection ────────────────────────────────────────────────────

-- Authoritative column definitions for every user-created table.
-- Used by: policy engine, query compiler, tables API.
CREATE TABLE IF NOT EXISTS flux_internal.table_metadata (
    id          UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    schema_name TEXT        NOT NULL,
    table_name  TEXT        NOT NULL,
    columns     JSONB       NOT NULL DEFAULT '[]',
    description TEXT        NOT NULL DEFAULT '',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (schema_name, table_name)
);

CREATE INDEX IF NOT EXISTS idx_table_metadata_lookup
    ON flux_internal.table_metadata (schema_name);

-- Per-column extended metadata (Flux types, file refs, computed exprs).
CREATE TABLE IF NOT EXISTS flux_internal.column_metadata (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    schema_name     TEXT        NOT NULL,
    table_name      TEXT        NOT NULL,
    column_name     TEXT        NOT NULL,
    pg_type         TEXT        NOT NULL,
    -- Flux type: text|integer|boolean|uuid|file|computed|relation|…
    fb_type         TEXT        NOT NULL DEFAULT 'default',
    not_null        BOOLEAN     NOT NULL DEFAULT false,
    primary_key     BOOLEAN     NOT NULL DEFAULT false,
    unique_col      BOOLEAN     NOT NULL DEFAULT false,
    default_expr    TEXT,
    -- fb_type = 'file' fields
    file_visibility TEXT,
    file_accept     JSONB,
    -- fb_type = 'computed' field
    computed_expr   TEXT,
    ordinal         INT         NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (schema_name, table_name, column_name)
);

CREATE INDEX IF NOT EXISTS idx_column_metadata_table
    ON flux_internal.column_metadata (schema_name, table_name);

-- Foreign-key-style relationships between user tables.
-- Powers automatic JOINs in the relational query API.
CREATE TABLE IF NOT EXISTS flux_internal.relationships (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    schema_name  TEXT        NOT NULL,
    from_table   TEXT        NOT NULL,
    from_column  TEXT        NOT NULL,
    to_table     TEXT        NOT NULL,
    to_column    TEXT        NOT NULL,
    -- 'has_one' | 'has_many' | 'belongs_to' | 'many_to_many'
    relationship TEXT        NOT NULL DEFAULT 'has_many',
    alias        TEXT,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (schema_name, from_table, from_column, to_table, to_column)
);

CREATE INDEX IF NOT EXISTS idx_relationships_from
    ON flux_internal.relationships (schema_name, from_table);

-- Cached JSON snapshot of the full schema graph per schema.
-- Updated on every DDL change; serves GET /db/schema without touching pg_catalog.
CREATE TABLE IF NOT EXISTS flux_internal.schema_snapshots (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    schema_name   TEXT        NOT NULL UNIQUE,
    snapshot_json JSONB       NOT NULL DEFAULT '{}',
    version       BIGINT      NOT NULL DEFAULT 0,
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- BYODB: user-provided PostgreSQL connection metadata.
CREATE TABLE IF NOT EXISTS flux_internal.project_databases (
    id                         UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    db_name                    TEXT        NOT NULL UNIQUE,
    connection_url             TEXT        NOT NULL,
    -- Safety anchors: verified on every reconnect to prevent silent mis-routing.
    expected_system_identifier TEXT,
    expected_db_name           TEXT,
    created_at                 TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at                 TIMESTAMPTZ NOT NULL DEFAULT now()
);


-- ─── Access control ──────────────────────────────────────────────────────────

CREATE TABLE IF NOT EXISTS flux_internal.policies (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    table_name      TEXT        NOT NULL,
    role            TEXT        NOT NULL,
    -- 'select' | 'insert' | 'update' | 'delete' | '*'
    operation       TEXT        NOT NULL,
    allowed_columns JSONB       NOT NULL DEFAULT '[]',
    row_condition   TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (table_name, role, operation)
);

CREATE INDEX IF NOT EXISTS idx_policies_lookup
    ON flux_internal.policies (table_name, role, operation);


-- ─── Table hooks ─────────────────────────────────────────────────────────────

-- Lifecycle hooks: bind a function OR a transform expression to a table event.
-- event: before_insert | after_insert | before_update | after_update |
--        before_delete | after_delete
CREATE TABLE IF NOT EXISTS flux_internal.hooks (
    id             UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    table_name     TEXT        NOT NULL,
    event          TEXT        NOT NULL,
    -- nullable when transform_expr is used instead
    function_id    UUID,
    enabled        BOOLEAN     NOT NULL DEFAULT true,
    -- evaluated in Rust at request time; zero function-invocation overhead
    transform_expr JSONB,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (table_name, event),
    CONSTRAINT hooks_target_check CHECK (
        function_id IS NOT NULL OR transform_expr IS NOT NULL
    )
);

CREATE INDEX IF NOT EXISTS idx_hooks_lookup
    ON flux_internal.hooks (table_name, event)
    WHERE enabled = true;


-- ─── Events ──────────────────────────────────────────────────────────────────

-- Append-only event bus.  Populated by the data engine on every mutation.
-- Workers consume and fan out to subscriptions (webhooks, functions, queues).
CREATE TABLE IF NOT EXISTS flux_internal.events (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    event_type   TEXT        NOT NULL,   -- e.g. "users.inserted", "orders.*"
    table_name   TEXT        NOT NULL,
    record_id    TEXT,
    operation    TEXT        NOT NULL DEFAULT 'insert',
    payload      JSONB       NOT NULL DEFAULT '{}',
    request_id   TEXT,
    delivered_at TIMESTAMPTZ,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_events_undelivered
    ON flux_internal.events (created_at)
    WHERE delivered_at IS NULL;

-- Subscription registry: maps event patterns to dispatch targets.
CREATE TABLE IF NOT EXISTS flux_internal.event_subscriptions (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Pattern: exact | "{table}.*" | "*"
    event_pattern TEXT        NOT NULL,
    -- 'webhook' | 'function' | 'queue_job'
    target_type   TEXT        NOT NULL,
    target_config JSONB       NOT NULL DEFAULT '{}',
    max_attempts  INT         NOT NULL DEFAULT 5,
    enabled       BOOLEAN     NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (event_pattern, target_type, target_config)
);

CREATE INDEX IF NOT EXISTS idx_event_subscriptions_enabled
    ON flux_internal.event_subscriptions (enabled)
    WHERE enabled = TRUE;

-- One delivery row per (event × subscription).  Created before dispatch so
-- a crash leaves the row in 'pending' for the retry worker to pick up.
CREATE TABLE IF NOT EXISTS flux_internal.event_deliveries (
    id              UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    event_id        UUID        NOT NULL REFERENCES flux_internal.events(id) ON DELETE CASCADE,
    subscription_id UUID        NOT NULL REFERENCES flux_internal.event_subscriptions(id) ON DELETE CASCADE,
    -- 'pending' | 'success' | 'failed' | 'dead_letter'
    status          TEXT        NOT NULL DEFAULT 'pending',
    response_status INT,
    error_message   TEXT,
    attempt         INT         NOT NULL DEFAULT 1,
    retry_at        TIMESTAMPTZ,
    next_attempt_at TIMESTAMPTZ,
    dispatched_at   TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_event_deliveries_event
    ON flux_internal.event_deliveries (event_id);

CREATE INDEX IF NOT EXISTS idx_event_deliveries_pending
    ON flux_internal.event_deliveries (created_at)
    WHERE status = 'pending';

CREATE INDEX IF NOT EXISTS idx_event_deliveries_retry
    ON flux_internal.event_deliveries (next_attempt_at)
    WHERE status = 'failed' AND next_attempt_at IS NOT NULL;


-- ─── Cron scheduler ──────────────────────────────────────────────────────────

-- Scheduled triggers (standard 5-field cron syntax).
-- The data-engine cron worker evaluates next_run_at every tick.
CREATE TABLE IF NOT EXISTS flux_internal.cron_jobs (
    id            UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    name          TEXT        NOT NULL UNIQUE,
    schedule      TEXT        NOT NULL,
    -- 'function' | 'queue_job'
    action_type   TEXT        NOT NULL,
    action_config JSONB       NOT NULL DEFAULT '{}',
    enabled       BOOLEAN     NOT NULL DEFAULT TRUE,
    last_run_at   TIMESTAMPTZ,
    next_run_at   TIMESTAMPTZ,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_cron_jobs_due
    ON flux_internal.cron_jobs (next_run_at)
    WHERE enabled = TRUE AND next_run_at IS NOT NULL;

-- Execution history — one row per cron trigger, written before dispatch.
CREATE TABLE IF NOT EXISTS flux_internal.cron_job_runs (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    job_name     TEXT        NOT NULL,
    scheduled_at TIMESTAMPTZ NOT NULL,
    started_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    finished_at  TIMESTAMPTZ,
    -- 'running' | 'success' | 'failed'
    status       TEXT        NOT NULL DEFAULT 'running',
    error        TEXT,
    request_id   UUID,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_cron_job_runs_job_name
    ON flux_internal.cron_job_runs (job_name, created_at DESC);

CREATE INDEX IF NOT EXISTS idx_cron_job_runs_request_id
    ON flux_internal.cron_job_runs (request_id)
    WHERE request_id IS NOT NULL;


-- ─── State mutations (data-engine owned) ─────────────────────────────────────

-- Append-only mutation log.  Written atomically with the user data write in
-- the same transaction (rolling back data also rolls back the log).
-- Powers: flux state blame | flux state history | flux incident replay
CREATE TABLE IF NOT EXISTS flux_internal.state_mutations (
    id             BIGSERIAL    PRIMARY KEY,
    table_name     TEXT         NOT NULL,
    record_pk      JSONB        NOT NULL,
    operation      TEXT         NOT NULL CHECK (operation IN ('insert', 'update', 'delete')),
    before_state   JSONB,
    after_state    JSONB,
    version        BIGINT       NOT NULL DEFAULT 1,
    actor_id       TEXT,
    request_id     TEXT,
    span_id        TEXT,
    mutation_seq   BIGSERIAL,
    changed_fields TEXT[],
    schema_name    TEXT,
    mutation_ts    TIMESTAMPTZ  NOT NULL DEFAULT now(),
    created_at     TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- Row history (flux state history <table> --id <pk>)
CREATE INDEX IF NOT EXISTS idx_state_mutations_row
    ON flux_internal.state_mutations (table_name, record_pk);

-- Time-window incident replay (flux incident replay 15:00..15:05)
CREATE INDEX IF NOT EXISTS idx_state_mutations_time
    ON flux_internal.state_mutations (mutation_ts);

-- Intra-request span link (flux trace debug step-through)
CREATE INDEX IF NOT EXISTS idx_state_mutations_span
    ON flux_internal.state_mutations (request_id, span_id)
    WHERE span_id IS NOT NULL;

-- Deterministic request-ordered replay
CREATE INDEX IF NOT EXISTS idx_state_mutations_request_seq
    ON flux_internal.state_mutations (request_id, mutation_seq)
    WHERE request_id IS NOT NULL;

-- Table-filtered replay (flux incident replay --table users)
CREATE INDEX IF NOT EXISTS idx_state_mutations_request_table
    ON flux_internal.state_mutations (request_id, table_name)
    WHERE request_id IS NOT NULL;

CREATE INDEX IF NOT EXISTS idx_state_mutations_request_id
    ON flux_internal.state_mutations (request_id)
    WHERE request_id IS NOT NULL;

-- O(log N) blame lookup (latest mutation for a specific record)
CREATE INDEX IF NOT EXISTS idx_state_mutations_pk_latest
    ON flux_internal.state_mutations (table_name, record_pk, mutation_seq DESC);


-- ─── Data-engine trace requests ──────────────────────────────────────────────

-- Request envelopes recorded by the data engine for replay support.
-- Separate from flux.gateway_trace_requests (gateway owns that one).
CREATE TABLE IF NOT EXISTS flux_internal.trace_requests (
    request_id      TEXT        PRIMARY KEY,
    method          TEXT        NOT NULL,
    path            TEXT        NOT NULL,
    headers         JSONB,
    body            JSONB,
    response_status INT,
    response_body   JSONB,
    duration_ms     INT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS idx_de_trace_requests_created
    ON flux_internal.trace_requests (created_at DESC);


-- ─── Outbound network call log (runtime-owned) ───────────────────────────────

-- Append-only log of every ctx.fetch() call made by a user function.
-- Written by the runtime immediately after the response is received.
-- Powers: flux trace <id> (http_fetch spans), flux incident replay (mock mode),
--         resume-from-checkpoint (know exactly which external calls already ran).
--
-- call_seq is the strictly-ordered sequence of the call within the request —
-- used to replay calls in the same order and to detect which calls must be
-- re-issued vs which can be skipped because they already succeeded.
CREATE TABLE IF NOT EXISTS flux_internal.network_calls (
    id              BIGSERIAL    PRIMARY KEY,
    request_id      TEXT         NOT NULL,
    span_id         TEXT,
    call_seq        INT          NOT NULL DEFAULT 0,  -- order within the request
    method          TEXT         NOT NULL,
    url             TEXT         NOT NULL,
    host            TEXT         NOT NULL,            -- extracted from url, for grouping
    request_headers JSONB,
    request_body    TEXT,
    status          INT,                              -- NULL if connection failed
    response_headers JSONB,
    response_body   TEXT,
    duration_ms     INT          NOT NULL,
    error           TEXT,                             -- non-null if the call threw
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT now()
);

-- Primary replay/trace lookup: all calls for a request in order
CREATE INDEX IF NOT EXISTS idx_network_calls_request
    ON flux_internal.network_calls (request_id, call_seq);

-- Host-based analysis: N+1 external calls, slow hosts, circuit breaker review
CREATE INDEX IF NOT EXISTS idx_network_calls_host
    ON flux_internal.network_calls (host, created_at DESC);

-- Time-window analysis (incident replay window)
CREATE INDEX IF NOT EXISTS idx_network_calls_created
    ON flux_internal.network_calls (created_at DESC);


-- ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
-- Triggers and notify functions
-- ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

-- ─── Gateway: route-change notify ────────────────────────────────────────────
-- Fires on any INSERT / UPDATE / DELETE on flux.routes.
-- The gateway's Postgres LISTEN loop picks this up and refreshes its
-- in-memory route snapshot immediately (no polling delay).

CREATE OR REPLACE FUNCTION notify_route_change()
RETURNS trigger AS $$
DECLARE
    payload text;
BEGIN
    payload := TG_OP || ':' || COALESCE(NEW.id::text, OLD.id::text);
    PERFORM pg_notify('route_changes', payload);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS route_change_notify ON flux.routes;
CREATE TRIGGER route_change_notify
    AFTER INSERT OR UPDATE OR DELETE ON flux.routes
    FOR EACH ROW EXECUTE FUNCTION notify_route_change();

-- ─── Queue: binding-change notify ────────────────────────────────────────────
-- Fires on any change to flux.queue_bindings so the queue worker can refresh
-- its in-memory (queue_name → function_id) map without a restart.

CREATE OR REPLACE FUNCTION flux.notify_queue_bindings_changed()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify('queue_bindings_changed', TG_OP);
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_queue_bindings_changed ON flux.queue_bindings;
CREATE TRIGGER trg_queue_bindings_changed
    AFTER INSERT OR UPDATE OR DELETE ON flux.queue_bindings
    FOR EACH STATEMENT EXECUTE FUNCTION flux.notify_queue_bindings_changed();

-- ─── Data-engine: schema snapshot version bump ───────────────────────────────

CREATE OR REPLACE FUNCTION flux_internal.bump_schema_snapshot_version()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    NEW.version    := OLD.version + 1;
    NEW.updated_at := now();
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_schema_snapshot_version ON flux_internal.schema_snapshots;
CREATE OR REPLACE TRIGGER trg_schema_snapshot_version
    BEFORE UPDATE ON flux_internal.schema_snapshots
    FOR EACH ROW EXECUTE FUNCTION flux_internal.bump_schema_snapshot_version();

-- ─── Data-engine: cache invalidation via LISTEN/NOTIFY ───────────────────────
-- When multiple data-engine instances run (horizontal scaling), each holds
-- its own in-process Moka cache.  This trigger notifies all instances on any
-- DDL change so stale cache entries are evicted immediately.
--
-- Channel: flux_cache_changes
-- Payload shapes:
--   {"type":"table",  "schema":"...","table":"..."}  → invalidate_table
--   {"type":"schema", "schema":"..."}                → invalidate_schema
--   {"type":"policy"}                                → invalidate_policy
--   {"type":"all"}                                   → invalidate_all

CREATE OR REPLACE FUNCTION flux_internal.notify_cache_change()
RETURNS trigger AS $$
DECLARE
    payload text;
    rec     record;
BEGIN
    rec := COALESCE(NEW, OLD);

    IF TG_TABLE_NAME = 'policies' THEN
        payload := '{"type":"policy"}';
    ELSIF TG_TABLE_NAME IN ('table_metadata', 'column_metadata') THEN
        payload := jsonb_build_object(
            'type',   'table',
            'schema', rec.schema_name,
            'table',  rec.table_name
        )::text;
    ELSIF TG_TABLE_NAME = 'relationships' THEN
        payload := jsonb_build_object(
            'type',   'schema',
            'schema', rec.schema_name
        )::text;
    ELSE
        payload := '{"type":"all"}';
    END IF;

    PERFORM pg_notify('flux_cache_changes', payload);
    RETURN rec;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS trg_cache_invalidate_policies ON flux_internal.policies;
CREATE OR REPLACE TRIGGER trg_cache_invalidate_policies
    AFTER INSERT OR UPDATE OR DELETE ON flux_internal.policies
    FOR EACH ROW EXECUTE FUNCTION flux_internal.notify_cache_change();

DROP TRIGGER IF EXISTS trg_cache_invalidate_table_metadata ON flux_internal.table_metadata;
CREATE OR REPLACE TRIGGER trg_cache_invalidate_table_metadata
    AFTER INSERT OR UPDATE OR DELETE ON flux_internal.table_metadata
    FOR EACH ROW EXECUTE FUNCTION flux_internal.notify_cache_change();

DROP TRIGGER IF EXISTS trg_cache_invalidate_column_metadata ON flux_internal.column_metadata;
CREATE OR REPLACE TRIGGER trg_cache_invalidate_column_metadata
    AFTER INSERT OR UPDATE OR DELETE ON flux_internal.column_metadata
    FOR EACH ROW EXECUTE FUNCTION flux_internal.notify_cache_change();

DROP TRIGGER IF EXISTS trg_cache_invalidate_relationships ON flux_internal.relationships;
CREATE OR REPLACE TRIGGER trg_cache_invalidate_relationships
    AFTER INSERT OR UPDATE OR DELETE ON flux_internal.relationships
    FOR EACH ROW EXECUTE FUNCTION flux_internal.notify_cache_change();

DROP TRIGGER IF EXISTS trg_cache_invalidate_hooks ON flux_internal.hooks;
CREATE OR REPLACE TRIGGER trg_cache_invalidate_hooks
    AFTER INSERT OR UPDATE OR DELETE ON flux_internal.hooks
    FOR EACH ROW EXECUTE FUNCTION flux_internal.notify_cache_change();


-- ─── Execution records + checkpoints (replay/resume primitives) ────────────────
-- Created idempotently so migration reruns are safe.

-- execution_records: one row per inbound HTTP request handled by `flux serve`.
-- Created at request start (status='running'), updated at end (ok/error).
CREATE TABLE IF NOT EXISTS flux.execution_records (
    id              UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    -- Human-readable label: the request method + path, e.g. "POST /webhook"
    label           TEXT         NOT NULL DEFAULT '',
    -- Serialised input payload (request body JSON, or raw text if non-JSON)
    input           JSONB,
    -- Output written back to the caller (null while running)
    output          JSONB,
    -- Error message if status='error' (null on success)
    error           TEXT,
    status          TEXT         NOT NULL DEFAULT 'running'
                    CHECK (status IN ('running','ok','error','timeout')),
    -- SHA-256 of the source file loaded at boot (hex, first 16 chars)
    code_sha        TEXT         NOT NULL DEFAULT '',
    started_at      TIMESTAMPTZ  NOT NULL DEFAULT now(),
    -- null while the execution is still running
    duration_ms     INTEGER,
    -- Identifies the flux serve instance that handled this request
    instance_id     TEXT         NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_execution_records_started
    ON flux.execution_records (started_at DESC);
CREATE INDEX IF NOT EXISTS idx_execution_records_status
    ON flux.execution_records (status, started_at DESC);

-- checkpoints: every IO boundary crossing recorded as a (request, response) pair.
-- call_index is always sequential per execution_id, never reused.
-- Used to drive replay (inject recorded response by call_index) and resume
-- (fast-forward through recorded checkpoints, then go live).
CREATE TABLE IF NOT EXISTS flux.checkpoints (
    id              UUID         PRIMARY KEY DEFAULT gen_random_uuid(),
    execution_id    UUID         NOT NULL
                    REFERENCES flux.execution_records(id) ON DELETE CASCADE,
    -- 0-based, incrementing per execution. First fetch() = 0, second = 1, …
    call_index      INTEGER      NOT NULL,
    -- 'http' for fetch() calls, 'db' for ctx.db writes
    boundary        TEXT         NOT NULL CHECK (boundary IN ('http','db')),
    -- Serialised request: { url, method, headers, body } for http;
    --                      { query, params } for db
    request         BYTEA        NOT NULL,
    -- Serialised response: { status, headers, body } for http;
    --                       { rows } for db
    response        BYTEA        NOT NULL,
    started_at_ms   BIGINT       NOT NULL,
    duration_ms     INTEGER      NOT NULL DEFAULT 0,
    UNIQUE (execution_id, call_index)
);
CREATE INDEX IF NOT EXISTS idx_checkpoints_execution
    ON flux.checkpoints (execution_id, call_index);
