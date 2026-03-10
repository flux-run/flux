/// Trigger Router
///
/// Receives raw trigger events (from gateway or scheduler) and resolves them
/// to function IDs + input payloads.
///
/// For Phase 1 the router is simple: map + pass-through.
/// For Phase 2 it will handle retry logic, event enrichment, and fan-out.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::registry::{TriggerRegistry, TriggerKind};

/// A resolved trigger event ready for function execution.
#[derive(Debug, Serialize, Deserialize)]
pub struct ResolvedTrigger {
    /// Trigger ID that fired
    pub trigger_id:  String,
    /// Function to execute
    pub function_id: String,
    /// Input payload to pass to the function
    pub payload:     Value,
    /// Source of the event (for tracing)
    pub source:      String,
    /// Tenant context
    pub tenant_id:   String,
    pub project_id:  Option<String>,
}

/// An incoming trigger event from the gateway.
#[derive(Debug, Deserialize)]
pub struct IncomingEvent {
    /// "webhook", "http", "cron"
    pub kind:        String,
    /// For webhooks: the source service ("stripe", "github", etc.)
    pub source:      Option<String>,
    /// Raw body of the incoming request
    pub payload:     Value,
    /// Headers from the original request (for signature verification)
    pub headers:     Option<Value>,
}

pub struct TriggerRouter {
    registry: TriggerRegistry,
}

impl TriggerRouter {
    pub fn new(registry: TriggerRegistry) -> Self {
        Self { registry }
    }

    /// Route an incoming event to one or more function executions.
    ///
    /// Returns a list of resolved triggers — there can be multiple functions
    /// listening to the same webhook source.
    pub fn route(&self, event: IncomingEvent) -> Vec<ResolvedTrigger> {
        match event.kind.as_str() {
            "webhook" => {
                let source = event.source.as_deref().unwrap_or("unknown");
                self.registry
                    .for_webhook(source)
                    .into_iter()
                    .map(|config| ResolvedTrigger {
                        trigger_id:  config.id.clone(),
                        function_id: config.function_id.clone(),
                        payload:     self.enrich_webhook_payload(&event.payload, source),
                        source:      format!("trigger:webhook.{}", source),
                        tenant_id:   config.tenant_id.clone(),
                        project_id:  config.project_id.clone(),
                    })
                    .collect()
            }

            "cron" => {
                // Cron events are pre-resolved: they include function_id directly
                let function_id = event.payload
                    .get("function_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                let tenant_id = event.payload
                    .get("tenant_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();

                if function_id.is_empty() {
                    return vec![];
                }

                vec![ResolvedTrigger {
                    trigger_id:  format!("cron:{}", function_id),
                    function_id,
                    payload:     event.payload,
                    source:      "trigger:cron".to_string(),
                    tenant_id,
                    project_id:  None,
                }]
            }

            _ => vec![], // "http" triggers are handled directly by the gateway
        }
    }

    /// Enrich the raw webhook body with Fluxbase metadata.
    fn enrich_webhook_payload(&self, raw: &Value, source: &str) -> Value {
        serde_json::json!({
            "trigger_source": source,
            "event":          raw,
        })
    }
}
