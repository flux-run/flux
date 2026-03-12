-- Storage provider configuration per project.
--
-- Supports:
--   'fluxbase'  — Fluxbase-managed bucket (default, no credentials needed)
--   'aws_s3'    — AWS S3
--   'r2'        — Cloudflare R2
--   'gcs'       — Google Cloud Storage (S3-compatible interop)
--   'minio'     — Self-hosted MinIO or any S3-compatible endpoint
--   'do_spaces' — DigitalOcean Spaces
--
-- Credentials (access_key_id, secret_access_key) are stored encrypted using the
-- same AES-GCM scheme used for project secrets (see api/src/secrets/encryption.rs).
-- The raw values are NEVER stored; only the ciphertext is persisted.

CREATE TABLE project_storage_providers (
    id                      UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    project_id              UUID        NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    tenant_id               UUID        NOT NULL,

    -- Provider type
    provider                VARCHAR(32) NOT NULL DEFAULT 'fluxbase',
    -- Constraint catches typos early rather than surfacing them at presign time
    CONSTRAINT chk_provider CHECK (provider IN ('fluxbase','aws_s3','r2','gcs','minio','do_spaces')),

    -- Bucket / endpoint details
    bucket_name             TEXT,
    region                  TEXT,
    -- Custom endpoint URL — required for R2, MinIO, DO Spaces, GCS interop.
    -- NULL for standard AWS S3 (SDK resolves from region).
    endpoint_url            TEXT,
    -- Optional prefix applied to all object keys: e.g. "my-app/production"
    base_path               TEXT,

    -- Encrypted credentials (NULL for 'fluxbase' provider)
    access_key_id_enc       TEXT,
    secret_access_key_enc   TEXT,

    -- When false, presign calls fall back to the Fluxbase-managed bucket.
    is_active               BOOLEAN     NOT NULL DEFAULT true,

    created_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at              TIMESTAMPTZ NOT NULL DEFAULT NOW(),

    -- One provider config per project
    UNIQUE (project_id)
);

CREATE INDEX idx_storage_providers_project ON project_storage_providers (project_id);
CREATE INDEX idx_storage_providers_tenant  ON project_storage_providers (tenant_id);
