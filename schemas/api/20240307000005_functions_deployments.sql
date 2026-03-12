-- 7. Secrets v2 – add scope column, rename encrypted_value -> value (plain for now; encrypt at app layer)
ALTER TABLE secrets ADD COLUMN IF NOT EXISTS scope TEXT NOT NULL DEFAULT 'project';
ALTER TABLE secrets ADD COLUMN IF NOT EXISTS value TEXT;
UPDATE secrets SET value = encrypted_value WHERE value IS NULL;
ALTER TABLE secrets ADD CONSTRAINT secrets_project_key_unique UNIQUE (project_id, key);

-- 8. Functions (metadata only; code lives in object storage)
CREATE TABLE IF NOT EXISTS functions (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    tenant_id   UUID NOT NULL REFERENCES tenants(id) ON DELETE CASCADE,
    project_id  UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    runtime     TEXT NOT NULL DEFAULT 'deno',
    created_at  TIMESTAMP DEFAULT NOW(),
    UNIQUE (project_id, name)
);

-- 9. Deployments (versioned artifacts per function)
CREATE TABLE IF NOT EXISTS deployments (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    function_id UUID NOT NULL REFERENCES functions(id) ON DELETE CASCADE,
    storage_key TEXT NOT NULL,
    version     INT NOT NULL DEFAULT 1,
    is_active   BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMP DEFAULT NOW()
);
