-- Drop old stub table if exists with wrong schema
DROP TABLE IF EXISTS flux.api_keys;

CREATE TABLE IF NOT EXISTS flux.api_keys (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id  UUID NOT NULL,
    name        TEXT NOT NULL,
    key_hash    TEXT NOT NULL,
    key_prefix  TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    last_used_at TIMESTAMPTZ,
    revoked_at  TIMESTAMPTZ,
    UNIQUE (project_id, name)
);
