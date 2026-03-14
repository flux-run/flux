//! Core agent execution loop.
//!
//! Flow per turn:
//!   1. Call LLM with current message history + tool schemas.
//!   2. Record the step to platform_logs.
//!   3a. If ToolUse → check rules → dispatch to runtime → append messages → continue.
//!   3b. If FinalAnswer → return output.
//!   4. If max_turns exhausted → return MaxTurnsExceeded error.

use serde_json::Value;
use uuid::Uuid;

use job_contract::dispatch::{ExecuteRequest, RuntimeDispatch};

use crate::llm::{ChatMessage, ChatRequest, LlmResponse};
use crate::recording::{record_step, StepRecord};
use crate::rules::RuleState;
use crate::schema::AgentDefinition;
use crate::AgentError;

pub struct AgentResult {
    pub output: String,
    pub turns:  u32,
}

pub async fn run_agent(
    agent:            &AgentDefinition,
    input:            Value,
    request_id:       &str,
    project_id:       Uuid,
    llm_key:          &str,
    tool_schemas:     &[Value],
    runtime_dispatch: &dyn RuntimeDispatch,
    pool:             &sqlx::PgPool,
) -> Result<Value, AgentError> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(90))
        .build()
        .map_err(|e| AgentError::Llm(e.to_string()))?;

    let mut messages: Vec<ChatMessage> = vec![
        ChatMessage::system(&agent.system),
        ChatMessage::user(&input.to_string()),
    ];

    let mut rules = RuleState::new(&agent.rules);
    let (top_p, max_tokens) = agent.config.as_ref()
        .map(|c| (c.top_p, c.max_tokens))
        .unwrap_or((None, None));

    for turn in 0..agent.max_turns {
        // ── Call LLM ─────────────────────────────────────────────────────
        let llm_resp = crate::llm::chat(
            &http,
            &agent.llm_url,
            llm_key,
            ChatRequest {
                model:       &agent.model,
                messages:    &messages,
                tools:       tool_schemas,
                temperature: agent.temperature,
                top_p,
                max_tokens,
            },
        )
        .await
        .map_err(AgentError::Llm)?;

        match llm_resp {
            // ── Tool call(s) ─────────────────────────────────────────────
            LlmResponse::ToolUse { calls, prompt_tokens, completion_tokens } => {
                // Record turn to platform_logs
                let tool_names = calls.iter()
                    .map(|c| c.function.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");

                let _ = record_step(pool, &StepRecord {
                    request_id, project_id,
                    agent_name:        &agent.name,
                    model:             &agent.model,
                    turn,
                    tool_choice:       Some(&tool_names),
                    prompt_tokens,
                    completion_tokens,
                    level:             "info",
                    message:           &format!("turn {} → tools: {}", turn + 1, tool_names),
                }).await;

                // Append the assistant tool-use message
                messages.push(ChatMessage::assistant_tool_calls(calls.clone()));

                // Execute each tool call in sequence
                for call in &calls {
                    let tool_name = &call.function.name;

                    // ── Guard-rail check ─────────────────────────────────
                    rules.check(tool_name).map_err(AgentError::RuleViolation)?;

                    // ── Parse arguments ──────────────────────────────────
                    let payload: Value = serde_json::from_str(&call.function.arguments)
                        .unwrap_or(serde_json::json!({}));

                    tracing::debug!(
                        agent = %agent.name,
                        turn,
                        tool = tool_name,
                        "dispatching tool"
                    );

                    // ── Dispatch to runtime ──────────────────────────────
                    let exec_resp = runtime_dispatch.execute(ExecuteRequest {
                        function_id:    tool_name.clone(),
                        project_id:     Some(project_id),
                        payload,
                        execution_seed: None,
                        request_id:     Some(request_id.to_string()),
                        parent_span_id: None,
                        runtime_hint:   None,
                        user_id:        None,
                        jwt_claims:     None,
                    })
                    .await
                    .map_err(|e| AgentError::Dispatch(format!("{}: {}", tool_name, e)))?;

                    rules.record(tool_name);

                    // Serialise tool output for the next LLM turn
                    let output_str = serde_json::to_string(&exec_resp.body)
                        .unwrap_or_else(|_| exec_resp.body.to_string());

                    messages.push(ChatMessage::tool_result(
                        &call.id, tool_name, &output_str,
                    ));
                }
            }

            // ── Final answer ─────────────────────────────────────────────
            LlmResponse::FinalAnswer { content, prompt_tokens, completion_tokens } => {
                let _ = record_step(pool, &StepRecord {
                    request_id, project_id,
                    agent_name:        &agent.name,
                    model:             &agent.model,
                    turn,
                    tool_choice:       None,
                    prompt_tokens,
                    completion_tokens,
                    level:             "info",
                    message:           &format!("turn {} → final answer", turn + 1),
                }).await;

                messages.push(ChatMessage::assistant_text(&content));

                tracing::info!(
                    agent = %agent.name,
                    turns = turn + 1,
                    "agent completed"
                );

                // Return structured output if the answer is valid JSON,
                // otherwise wrap in { "answer": "..." }
                let output: Value = serde_json::from_str(&content)
                    .unwrap_or_else(|_| serde_json::json!({ "answer": content }));

                return Ok(output);
            }
        }
    }

    Err(AgentError::MaxTurnsExceeded(agent.max_turns))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use uuid::Uuid;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    use wiremock::matchers::{method, path};

    use job_contract::dispatch::{ExecuteRequest, ExecuteResponse, RuntimeDispatch};

    use crate::schema::{AgentDefinition, Rule};
    use crate::AgentError;

    // ── Helpers ───────────────────────────────────────────────────────────

    /// Mock RuntimeDispatch that records calls and returns a canned response.
    struct MockRuntime {
        response:  serde_json::Value,
        call_log:  Arc<Mutex<Vec<String>>>,
    }

    impl MockRuntime {
        fn new(response: serde_json::Value) -> (Self, Arc<Mutex<Vec<String>>>) {
            let log = Arc::new(Mutex::new(vec![]));
            (Self { response, call_log: Arc::clone(&log) }, log)
        }
    }

    #[async_trait]
    impl RuntimeDispatch for MockRuntime {
        async fn execute(&self, req: ExecuteRequest) -> Result<ExecuteResponse, String> {
            self.call_log.lock().unwrap().push(req.function_id.clone());
            Ok(ExecuteResponse {
                body:        self.response.clone(),
                status:      200,
                duration_ms: 0,
            })
        }
    }

    /// Build a minimal `AgentDefinition` pointing at `llm_url`.
    fn test_agent(llm_url: &str) -> AgentDefinition {
        AgentDefinition {
            name:         "test-agent".to_string(),
            model:        "gpt-test".to_string(),
            system:       "You are a test agent.".to_string(),
            tools:        vec![],
            llm_url:      llm_url.to_string(),
            llm_secret:   "OPENAI_KEY".to_string(),
            max_turns:    5,
            temperature:  0.0,
            config:       None,
            input_schema: None,
            output_schema:None,
            rules:        vec![],
        }
    }

    /// Dummy PgPool that satisfies the type but never actually connects.
    /// `record_step` errors are silently swallowed by the loop, so this is safe.
    fn fake_pool() -> sqlx::PgPool {
        sqlx::PgPool::connect_lazy("postgres://invalid:invalid@127.0.0.1:1/fake")
            .expect("lazy pool must be created without connecting")
    }

    /// Build an OpenAI-format final-answer LLM response body.
    fn llm_final(content: &str) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": { "role": "assistant", "content": content },
                "finish_reason": "stop"
            }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
        })
    }

    /// Build an OpenAI-format tool-call LLM response body.
    fn llm_tool_call(call_id: &str, fn_name: &str, args: &str) -> serde_json::Value {
        serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{
                        "id": call_id,
                        "type": "function",
                        "function": { "name": fn_name, "arguments": args }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": { "prompt_tokens": 15, "completion_tokens": 8 }
        })
    }

    // ── Tests ─────────────────────────────────────────────────────────────

    #[tokio::test]
    async fn final_answer_returned_immediately() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(llm_final("Hello!")))
            .mount(&server).await;

        let agent = test_agent(&format!("{}/v1/chat/completions", server.uri()));
        let (runtime, _log) = MockRuntime::new(serde_json::json!({}));

        let result = run_agent(
            &agent,
            serde_json::json!({"task": "greet"}),
            "req-1",
            Uuid::new_v4(),
            "test-api-key",
            &[],
            &runtime,
            &fake_pool(),
        ).await.unwrap();

        // Plain text → wrapped in {"answer": ...}
        assert_eq!(result["answer"], "Hello!");
    }

    #[tokio::test]
    async fn json_final_answer_preserved_as_is() {
        let server = MockServer::start().await;
        let json_response = r#"{"status":"ok","count":3}"#;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(llm_final(json_response)))
            .mount(&server).await;

        let agent = test_agent(&format!("{}/v1/chat/completions", server.uri()));
        let (runtime, _log) = MockRuntime::new(serde_json::json!({}));

        let result = run_agent(
            &agent,
            serde_json::json!({}),
            "req-2",
            Uuid::new_v4(),
            "key",
            &[],
            &runtime,
            &fake_pool(),
        ).await.unwrap();

        // Valid JSON answer must be returned as-is (not wrapped)
        assert_eq!(result["status"], "ok");
        assert_eq!(result["count"],  3);
    }

    #[tokio::test]
    async fn one_tool_call_then_final_answer() {
        let server = MockServer::start().await;

        // First LLM call → tool use
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200)
                .set_body_json(llm_tool_call("call_1", "search", r#"{"q":"flux"}"#)))
            .up_to_n_times(1)
            .mount(&server).await;

        // Second LLM call → final answer
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200)
                .set_body_json(llm_final("Search done.")))
            .mount(&server).await;

        let agent = test_agent(&format!("{}/v1/chat/completions", server.uri()));
        let (runtime, call_log) = MockRuntime::new(serde_json::json!({"results": []}));

        let result = run_agent(
            &agent,
            serde_json::json!({}),
            "req-3",
            Uuid::new_v4(),
            "key",
            &[],
            &runtime,
            &fake_pool(),
        ).await.unwrap();

        // Tool must have been dispatched once
        let log = call_log.lock().unwrap();
        assert_eq!(log.as_slice(), &["search"]);
        drop(log);

        // Final answer wraps text
        assert_eq!(result["answer"], "Search done.");
    }

    #[tokio::test]
    async fn rule_violation_aborts_loop() {
        let server = MockServer::start().await;
        // LLM wants to call "notify" but the rule says "require create_issue first"
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200)
                .set_body_json(llm_tool_call("call_x", "notify", r#"{}"#)))
            .mount(&server).await;

        let mut agent = test_agent(&format!("{}/v1/chat/completions", server.uri()));
        agent.rules = vec![Rule::Require {
            before:  "notify".to_string(),
            require: "create_issue".to_string(),
        }];

        let (runtime, _log) = MockRuntime::new(serde_json::json!({}));

        let err = run_agent(
            &agent,
            serde_json::json!({}),
            "req-4",
            Uuid::new_v4(),
            "key",
            &[],
            &runtime,
            &fake_pool(),
        ).await.unwrap_err();

        assert!(
            matches!(err, AgentError::RuleViolation(_)),
            "expected RuleViolation, got: {:?}", err
        );
    }

    #[tokio::test]
    async fn max_turns_exceeded_returns_error() {
        let server = MockServer::start().await;
        // LLM always returns a tool call — loop never gets a final answer.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200)
                .set_body_json(llm_tool_call("call_n", "tool_a", r#"{}"#)))
            .mount(&server).await;

        let mut agent = test_agent(&format!("{}/v1/chat/completions", server.uri()));
        agent.max_turns = 2; // exhaust quickly

        let (runtime, _log) = MockRuntime::new(serde_json::json!({}));

        let err = run_agent(
            &agent,
            serde_json::json!({}),
            "req-5",
            Uuid::new_v4(),
            "key",
            &[],
            &runtime,
            &fake_pool(),
        ).await.unwrap_err();

        assert!(
            matches!(err, AgentError::MaxTurnsExceeded(2)),
            "expected MaxTurnsExceeded(2), got: {:?}", err
        );
    }

    #[tokio::test]
    async fn llm_auth_error_propagates() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Unauthorized"))
            .mount(&server).await;

        let agent = test_agent(&format!("{}/v1/chat/completions", server.uri()));
        let (runtime, _) = MockRuntime::new(serde_json::json!({}));

        let err = run_agent(
            &agent,
            serde_json::json!({}),
            "req-6",
            Uuid::new_v4(),
            "bad-key",
            &[],
            &runtime,
            &fake_pool(),
        ).await.unwrap_err();

        assert!(matches!(err, AgentError::Llm(_)), "expected Llm error, got: {:?}", err);
    }
}
