use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteRequest {
    pub function_id: String,
    pub payload: Value,
    pub execution_seed: Option<i64>,
    pub request_id: Option<String>,
    pub parent_span_id: Option<String>,
    pub runtime_hint: Option<String>,
    pub user_id: Option<String>,
    pub jwt_claims: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteResponse {
    pub body: Value,
    pub status: u16,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedFunction {
    pub function_id: Uuid,
}

#[async_trait]
pub trait RuntimeDispatch: Send + Sync {
    async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteResponse, String>;
}

#[async_trait]
pub trait ApiDispatch: Send + Sync {
    async fn get_bundle(&self, function_id: &str) -> Result<Value, String>;
    async fn write_log(&self, entry: Value) -> Result<(), String>;
    async fn write_network_call(&self, call: Value) -> Result<(), String>;
    async fn get_secrets(&self) -> Result<HashMap<String, String>, String>;
    async fn resolve_function(&self, name: &str) -> Result<ResolvedFunction, String>;
}

#[async_trait]
pub trait DataEngineDispatch: Send + Sync {
    async fn execute_sql(
        &self,
        sql: String,
        params: Vec<Value>,
        database: String,
        request_id: String,
    ) -> Result<Value, String>;
}
