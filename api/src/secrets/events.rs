/// Emit a structured audit event for secret lifecycle operations.
///
/// Writes to the structured tracing pipeline. An external event bus
/// integration (Kafka/Redis Streams) can be added here without changing callers.
pub fn emit_secret_event(event_type: &str, key: &str, version: i32) {
    tracing::info!(
        event_type,
        secret_key = key,
        version,
        "secret lifecycle event",
    );
}
