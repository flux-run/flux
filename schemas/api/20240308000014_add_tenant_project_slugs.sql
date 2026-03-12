-- Migration: Add slug to tenants and projects
ALTER TABLE tenants ADD COLUMN slug TEXT;
ALTER TABLE projects ADD COLUMN slug TEXT;

-- Initial backfill based on name for tenants
-- Append first 4 chars of ID to ensure uniqueness in case of name collisions
UPDATE tenants SET slug = lower(regexp_replace(name, '[^a-zA-Z0-9]+', '-', 'g')) || '-' || substring(id::text from 1 for 4) WHERE slug IS NULL;
UPDATE tenants SET slug = trim(both '-' from slug);

-- Initial backfill based on name for projects
UPDATE projects SET slug = lower(regexp_replace(name, '[^a-zA-Z0-9]+', '-', 'g')) || '-' || substring(id::text from 1 for 4) WHERE slug IS NULL;
UPDATE projects SET slug = trim(both '-' from slug);

-- Make them NOT NULL
ALTER TABLE tenants ALTER COLUMN slug SET NOT NULL;
ALTER TABLE projects ALTER COLUMN slug SET NOT NULL;

-- Add UNIQUE constraints
ALTER TABLE tenants ADD CONSTRAINT tenants_slug_key UNIQUE (slug);
-- For projects, slug should ideally be unique per tenant, 
-- but global uniqueness is easier for initial routing logic.
-- Let's stick with global uniqueness for now to keep the gateway simple.
ALTER TABLE projects ADD CONSTRAINT projects_slug_key UNIQUE (slug);
