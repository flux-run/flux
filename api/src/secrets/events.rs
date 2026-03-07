use uuid::Uuid;

pub fn emit_secret_event(event_type: &str, tenant_id: Uuid, project_id: Option<Uuid>, key: &str, version: i32) {
    // TODO: Publish to actual real event bus (e.g. Kafka/Redis)
    let project_str = match project_id {
        Some(id) => format!(r#""{}""#, id),
        None => "null".to_string()
    };
    
    println!(
        r#"{{"event": "{}", "tenant_id": "{}", "project_id": {}, "key": "{}", "version": {}}}"#,
        event_type, tenant_id, project_str, key, version
    );
}
