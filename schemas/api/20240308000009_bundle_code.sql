-- Add bundle_code column to store JS bundle bytes inline (MVP, no S3)
ALTER TABLE deployments ADD COLUMN IF NOT EXISTS bundle_code TEXT;

-- Add status column (referenced in code but missing from original migration)
ALTER TABLE deployments ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'ready';
