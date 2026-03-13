-- Allow hooks to store either a deployed function (function_id) or a compiled
-- TypeScript transform (transform_expr JSONB).  The two are mutually exclusive:
-- transform_expr hooks are evaluated in Rust by TransformExpr::apply() at
-- request time with zero function invocation overhead.
--
-- function_id is made nullable so rows with only transform_expr are valid.

ALTER TABLE fluxbase_internal.hooks
    ALTER COLUMN function_id DROP NOT NULL,
    ADD COLUMN IF NOT EXISTS transform_expr JSONB;

-- Constraint: at least one of function_id / transform_expr must be set.
ALTER TABLE fluxbase_internal.hooks
    ADD CONSTRAINT hooks_target_check
    CHECK (function_id IS NOT NULL OR transform_expr IS NOT NULL);
