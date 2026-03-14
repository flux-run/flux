-- Queue bindings: link a named queue to the function that consumes it.
--
-- A binding says "when a job is enqueued on <queue_name>, dispatch it to
-- <function_id>".  The queue worker reads this table on startup and refreshes
-- it via LISTEN/NOTIFY on channel `queue_bindings_changed`.
--
-- One queue may have at most one consumer function (UNIQUE constraint).

CREATE TABLE IF NOT EXISTS flux.queue_bindings (
    id           UUID        PRIMARY KEY DEFAULT gen_random_uuid(),
    queue_name   TEXT        NOT NULL,
    function_id  UUID        NOT NULL REFERENCES functions(id) ON DELETE CASCADE,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),

    UNIQUE (queue_name, function_id)
);

CREATE INDEX IF NOT EXISTS idx_queue_bindings_queue_name
    ON flux.queue_bindings (queue_name);

-- Notify the queue worker whenever bindings change.
CREATE OR REPLACE FUNCTION flux.notify_queue_bindings_changed()
RETURNS TRIGGER LANGUAGE plpgsql AS $$
BEGIN
    PERFORM pg_notify('queue_bindings_changed', TG_OP);
    RETURN NEW;
END;
$$;

DROP TRIGGER IF EXISTS trg_queue_bindings_changed ON flux.queue_bindings;
CREATE TRIGGER trg_queue_bindings_changed
    AFTER INSERT OR UPDATE OR DELETE ON flux.queue_bindings
    FOR EACH STATEMENT EXECUTE FUNCTION flux.notify_queue_bindings_changed();
