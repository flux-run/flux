-- Bundles are now served from the filesystem (FLUX_FUNCTIONS_DIR), not Postgres.
--
-- Every deployed function bundle (.js or .wasm) lives at:
--   {FLUX_FUNCTIONS_DIR}/{function_name}.js   (Deno/JS runtime)
--   {FLUX_FUNCTIONS_DIR}/{function_name}.wasm  (WASM runtime)
--
-- In development:  {project_root}/.flux/build/   (set by `flux dev`)
-- In production:   /app/functions/               (baked into Docker image at CI time)
--
-- Removing these blob columns keeps Postgres small and fast — only operational
-- data (executions, mutations, queue jobs, secrets) lives in the DB.

ALTER TABLE flux.deployments DROP COLUMN IF EXISTS bundle_code;
ALTER TABLE flux.deployments DROP COLUMN IF EXISTS bundle_url;
