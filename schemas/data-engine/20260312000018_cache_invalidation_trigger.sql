-- Cache invalidation via Postgres LISTEN/NOTIFY.
--
-- When running multiple data-engine instances (horizontal scaling), each
-- instance holds its own in-process Moka caches.  Without coordination a
-- DDL mutation handled by instance A leaves instance B's caches stale.
--
-- Solution: trigger a NOTIFY on every mutation to the five schema-affecting
-- tables.  Each data-engine instance runs a dedicated PgListener loop
-- (cache/invalidation.rs) and calls the appropriate CacheManager method
-- when a notification arrives.
--
-- Channel: flux_cache_changes
-- Payload shapes:
--   {"type":"table",  "schema":"main","table":"users"}  → invalidate_table
--   {"type":"schema", "schema":"main"}                  → invalidate_schema
--   {"type":"policy"}                                   → invalidate_policy
--   {"type":"all"}                                      → invalidate_all

-- ─── Trigger function ─────────────────────────────────────────────────────────

CREATE OR REPLACE FUNCTION fluxbase_internal.notify_cache_change()
RETURNS trigger AS $$
DECLARE
    payload text;
    rec     record;
BEGIN
    -- For DELETE triggers OLD is the row; for INSERT/UPDATE NEW is.
    rec := COALESCE(NEW, OLD);

    IF TG_TABLE_NAME = 'policies' THEN
        -- Policy evaluation results cached separately in RwLock<HashMap>.
        -- Plan cache entries keyed by policy_fingerprint become unreachable
        -- automatically (new policy → different fingerprint → cache miss),
        -- so only the policy cache needs flushing.
        payload := '{"type":"policy"}';

    ELSIF TG_TABLE_NAME IN ('table_metadata', 'column_metadata') THEN
        -- Schema cache (col_meta + relationships) and plan cache for this
        -- specific table only.
        payload := jsonb_build_object(
            'type',   'table',
            'schema', rec.schema_name,
            'table',  rec.table_name
        )::text;

    ELSIF TG_TABLE_NAME = 'relationships' THEN
        -- Relationships affect all queries that join across the schema,
        -- so evict every cache entry for the schema (not just one table).
        payload := jsonb_build_object(
            'type',   'schema',
            'schema', rec.schema_name
        )::text;

    ELSE
        -- hooks: no schema_name column — conservatively invalidate all
        -- schema + plan cache entries.  Policy cache is unaffected.
        payload := '{"type":"all"}';
    END IF;

    PERFORM pg_notify('flux_cache_changes', payload);
    RETURN rec;
END;
$$ LANGUAGE plpgsql;

-- ─── Triggers ─────────────────────────────────────────────────────────────────

-- AFTER is correct: fire only if the transaction commits.
-- FOR EACH ROW: we need the row values to build the targeted payload.

CREATE OR REPLACE TRIGGER trg_cache_invalidate_policies
    AFTER INSERT OR UPDATE OR DELETE ON fluxbase_internal.policies
    FOR EACH ROW EXECUTE FUNCTION fluxbase_internal.notify_cache_change();

CREATE OR REPLACE TRIGGER trg_cache_invalidate_table_metadata
    AFTER INSERT OR UPDATE OR DELETE ON fluxbase_internal.table_metadata
    FOR EACH ROW EXECUTE FUNCTION fluxbase_internal.notify_cache_change();

CREATE OR REPLACE TRIGGER trg_cache_invalidate_column_metadata
    AFTER INSERT OR UPDATE OR DELETE ON fluxbase_internal.column_metadata
    FOR EACH ROW EXECUTE FUNCTION fluxbase_internal.notify_cache_change();

CREATE OR REPLACE TRIGGER trg_cache_invalidate_relationships
    AFTER INSERT OR UPDATE OR DELETE ON fluxbase_internal.relationships
    FOR EACH ROW EXECUTE FUNCTION fluxbase_internal.notify_cache_change();

CREATE OR REPLACE TRIGGER trg_cache_invalidate_hooks
    AFTER INSERT OR UPDATE OR DELETE ON fluxbase_internal.hooks
    FOR EACH ROW EXECUTE FUNCTION fluxbase_internal.notify_cache_change();
