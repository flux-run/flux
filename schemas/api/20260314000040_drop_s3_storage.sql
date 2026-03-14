-- Remove S3/object storage remnants.
-- Bundles are now stored inline in deployments.bundle_code only.

-- Drop the bundle_url column (was used for S3 presigned URL keys)
ALTER TABLE flux.deployments DROP COLUMN IF EXISTS bundle_url;

-- Drop the storage providers table (no longer offering BYO S3)
DROP TABLE IF EXISTS flux.project_storage_providers;
DROP TABLE IF EXISTS project_storage_providers;
