-- Postgres LISTEN/NOTIFY trigger for gateway snapshot invalidation.
--
-- When any row in `routes` is inserted, updated, or deleted, this trigger
-- fires NOTIFY on the `route_changes` channel.  The gateway's listener
-- picks this up and refreshes its in-memory snapshot immediately rather
-- than waiting for the next 60-second poll cycle.
--
-- The payload carries the operation type and route ID for observability.
-- The gateway ignores the payload content and always does a full refresh.

CREATE OR REPLACE FUNCTION notify_route_change()
RETURNS trigger AS $$
DECLARE
    payload text;
BEGIN
    payload := TG_OP || ':' || COALESCE(NEW.id::text, OLD.id::text);
    PERFORM pg_notify('route_changes', payload);
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

DROP TRIGGER IF EXISTS route_change_notify ON flux.routes;

CREATE TRIGGER route_change_notify
AFTER INSERT OR UPDATE OR DELETE ON flux.routes
FOR EACH ROW EXECUTE FUNCTION notify_route_change();
