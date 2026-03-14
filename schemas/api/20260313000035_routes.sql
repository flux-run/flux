-- Route definitions — gateway reads this table to dispatch incoming requests.
--
-- Every `flux deploy` writes the [[routes]] from flux.toml into this table,
-- replacing the previous active set for the project. The gateway then picks
-- them up without a restart.
--
-- The old `routes` table (migration 013) was moved to the flux schema by
-- migration 028 but had an incompatible schema. Drop and recreate with the
-- canonical column set.

DROP TABLE IF EXISTS flux.routes CASCADE;

CREATE TABLE flux.routes (
    id                      UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id              UUID        NOT NULL REFERENCES flux.projects(id) ON DELETE CASCADE,

    -- HTTP path — may contain named params: /users/:id/activate
    path                    TEXT        NOT NULL,

    -- HTTP method — stored uppercase: GET, POST, PUT, PATCH, DELETE
    method                  TEXT        NOT NULL DEFAULT 'POST',

    -- Name of the function to invoke (matches flux.functions.name)
    function_name           TEXT        NOT NULL,

    -- Ordered list of middleware names to run before the function.
    -- e.g. ["auth", "require_admin"]
    middleware              JSONB       NOT NULL DEFAULT '[]',

    -- Per-route rate limit in requests per minute. NULL = project default.
    rate_limit_per_minute   INT,

    -- Links this route back to the project deployment that created it.
    -- Used to restore the exact route set on rollback.
    project_deployment_id   UUID        REFERENCES flux.project_deployments(id) ON DELETE SET NULL,

    -- Only one active route set per project. flux deploy flips is_active.
    is_active               BOOLEAN     NOT NULL DEFAULT TRUE,

    -- Auth / CORS / validation — configured per route in flux.toml
    auth_type               TEXT        NOT NULL DEFAULT 'none',
    cors_enabled            BOOLEAN     NOT NULL DEFAULT FALSE,
    jwks_url                TEXT,
    jwt_audience            TEXT,
    jwt_issuer              TEXT,
    json_schema             JSONB,
    cors_origins            TEXT[],
    cors_headers            TEXT[],

    created_at              TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Fast dispatch lookup: gateway queries (project_id, is_active=true) on every request.
CREATE INDEX idx_routes_project_active
    ON flux.routes (project_id, is_active)
    WHERE is_active = TRUE;

-- Enforce uniqueness of (project_id, method, path) among active routes.
CREATE UNIQUE INDEX idx_routes_project_method_path_active
    ON flux.routes (project_id, method, path)
    WHERE is_active = TRUE;

-- Re-attach the LISTEN/NOTIFY trigger (dropped with the old table above).
-- The notify_route_change() function was created in migration 20260312000029.
DROP TRIGGER IF EXISTS route_change_notify ON flux.routes;
CREATE TRIGGER route_change_notify
AFTER INSERT OR UPDATE OR DELETE ON flux.routes
FOR EACH ROW EXECUTE FUNCTION notify_route_change();
