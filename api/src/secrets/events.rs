pub fn emit_secret_event(event_type: &str, key: &str, version: i32) {
    // TODO: Publish to actual real event bus (e.g. Kafka/Redis)
    println!(
        r#"{{"event": "{}", "key": "{}", "version": {}}}"#,
        event_type, key, version
    );
}
