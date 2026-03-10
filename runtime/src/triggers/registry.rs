/// Trigger Registry
///
/// Stores trigger→function bindings. During function deployment, flux.json
/// is parsed and triggers are registered here. At runtime the trigger router
/// uses this registry to map incoming events to function IDs.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// The kind of event that fires a trigger.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerKind {
    /// Any HTTP POST/GET to the route. The gateway creates this automatically
    /// for every deployed function.
    Http,

    /// Authenticated webhook from an external service.
    /// `source` identifies the service (e.g. "stripe", "github").
    Webhook { source: String },

    /// Time-based schedule in cron expression format.
    /// e.g. "0 9 * * 1-5" = weekdays at 9am UTC
    Cron { schedule: String },
}

/// A registered trigger binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// Unique ID for this trigger binding
    pub id:          String,
    /// Function to invoke when triggered
    pub function_id: String,
    /// The trigger kind + configuration
    pub kind:        TriggerKind,
    /// Human label for dashboard display
    pub label:       Option<String>,
    /// Tenant this trigger belongs to
    pub tenant_id:   String,
    /// Project this trigger belongs to
    pub project_id:  Option<String>,
}

/// In-memory trigger registry.
///
/// In Phase 1 this is loaded from deployed function configs.
/// In future phases it persists in the database and reloads on deploy.
pub struct TriggerRegistry {
    /// trigger_id → TriggerConfig
    triggers:    HashMap<String, TriggerConfig>,
    /// webhook_source → Vec<trigger_id>  (fast lookup by webhook source)
    by_webhook:  HashMap<String, Vec<String>>,
    /// function_id → Vec<trigger_id>     (reverse lookup)
    by_function: HashMap<String, Vec<String>>,
}

impl TriggerRegistry {
    pub fn new() -> Self {
        Self {
            triggers:    HashMap::new(),
            by_webhook:  HashMap::new(),
            by_function: HashMap::new(),
        }
    }

    /// Register a trigger binding.
    pub fn register(&mut self, config: TriggerConfig) {
        // Index by webhook source for fast dispatch
        if let TriggerKind::Webhook { ref source } = config.kind {
            self.by_webhook
                .entry(source.clone())
                .or_default()
                .push(config.id.clone());
        }

        self.by_function
            .entry(config.function_id.clone())
            .or_default()
            .push(config.id.clone());

        self.triggers.insert(config.id.clone(), config);
    }

    /// Remove all triggers for a function (called on function delete/redeploy).
    pub fn deregister_function(&mut self, function_id: &str) {
        if let Some(ids) = self.by_function.remove(function_id) {
            for id in &ids {
                if let Some(config) = self.triggers.remove(id) {
                    if let TriggerKind::Webhook { ref source } = config.kind {
                        if let Some(list) = self.by_webhook.get_mut(source) {
                            list.retain(|i| i != id);
                        }
                    }
                }
            }
        }
    }

    /// Find triggers for an incoming webhook by source name.
    pub fn for_webhook(&self, source: &str) -> Vec<&TriggerConfig> {
        self.by_webhook
            .get(source)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.triggers.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// All triggers for a function.
    pub fn for_function(&self, function_id: &str) -> Vec<&TriggerConfig> {
        self.by_function
            .get(function_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.triggers.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// All registered triggers (for CLI and dashboard listing).
    pub fn all(&self) -> Vec<&TriggerConfig> {
        self.triggers.values().collect()
    }
}

impl Default for TriggerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
