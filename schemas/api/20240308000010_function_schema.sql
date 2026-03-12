-- Schema metadata for functions (enables workflow builder + AI agent integration)
ALTER TABLE functions ADD COLUMN IF NOT EXISTS description TEXT;
ALTER TABLE functions ADD COLUMN IF NOT EXISTS input_schema  JSONB;
ALTER TABLE functions ADD COLUMN IF NOT EXISTS output_schema JSONB;
