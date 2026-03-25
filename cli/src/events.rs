use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum FluxEvent {
    ExecutionStart {
        id: String,
        method: String,
        path: String,
        timestamp: String,
    },
    HttpRequest {
        method: String,
        path: String,
    },
    FetchStart {
        url: String,
        method: String,
    },
    FetchEnd {
        status: u16,
        duration_ms: u64,
    },
    DbQueryStart {
        query: String,
    },
    DbQueryEnd {
        duration_ms: u64,
    },
    Log {
        level: String,
        message: String,
    },
    Error {
        message: String,
        stack: Option<String>,
    },
    ExecutionEnd {
        id: String,
        status: String, // "ok" or "error"
        duration_ms: u64,
    },
}

impl FluxEvent {
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }
}
