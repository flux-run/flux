use deno_core::{JsRuntime, RuntimeOptions, OpState, Extension};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use tokio::time::{timeout, Duration};

use crate::tools::executor::ToolOpState;
use crate::tools::registry::ToolRegistry;
use crate::tools::composio;
use crate::agent::AgentOpState;

// ── Tool execution Deno op ────────────────────────────────────────────────────
//
// This is the bridge between the JS sandbox and the Rust tool executor.
// ctx.tools.run("slack.send_message", { ... }) calls this op.
// One execution path: Function → op_execute_tool → ToolExecutor → Composio.

#[deno_core::op2(async)]
#[serde]
pub async fn op_execute_tool(
    state: Rc<RefCell<OpState>>,
    #[string] tool_name: String,
    #[serde] input: serde_json::Value,
) -> Result<serde_json::Value, std::io::Error> {
    // Clone state before await boundary (Rc<RefCell> cannot cross await)
    let (api_key, entity_id) = {
        let s = state.borrow();
        let ts = s.borrow::<ToolOpState>();
        (ts.api_key.clone(), ts.entity_id.clone())
    };

    let registry = ToolRegistry::new();
    let composio_action = registry.resolve_composio_action(&tool_name);
    let start = std::time::Instant::now();

    let api_key_str = api_key.as_deref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "tools_not_configured: Composio integration is not available on this runtime",
        )
    })?;

    let result = composio::execute_action(api_key_str, &entity_id, &composio_action, input)
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let duration_ms = start.elapsed().as_millis() as u64;
    let mut response = serde_json::json!({
        "_tool":        &tool_name,
        "_duration_ms": duration_ms,
        "_success":     true,
    });
    if let Some(data) = result.data {
        if let serde_json::Value::Object(map) = data {
            if let serde_json::Value::Object(ref mut resp_map) = response {
                resp_map.extend(map);
            }
        }
    }
    Ok(response)
}

// ── Agent LLM op ──────────────────────────────────────────────────────────────
//
// Deno bridge: JS calls Deno.core.ops.op_agent_llm_call(messages, toolDefs)
// from inside ctx.agent.run().
// The op reads AgentOpState (api_key, url, model) from Deno OpState,
// calls the LLM via agent::llm::call_llm, and returns the action decision.

#[deno_core::op2(async)]
#[serde]
pub async fn op_agent_llm_call(
    state:      Rc<RefCell<OpState>>,
    #[serde] messages:  serde_json::Value,
    #[serde] _tool_defs: serde_json::Value,
) -> Result<serde_json::Value, std::io::Error> {
    let (llm_key, llm_url, llm_model) = {
        let s = state.borrow();
        let ts = s.borrow::<AgentOpState>();
        (ts.llm_key.clone(), ts.llm_url.clone(), ts.llm_model.clone())
    };

    let api_key = llm_key.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "agent_not_configured: FLUXBASE_LLM_KEY secret not set. \
             Add it in your Fluxbase dashboard → Secrets.",
        )
    })?;

    crate::agent::llm::call_llm(&api_key, &llm_url, &llm_model, messages)
        .await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))
}

/// Build the Fluxbase runtime extension — tools + agent ops.
fn build_fluxbase_extension() -> Extension {
    Extension {
        name: "fluxbase",
        ops: Cow::Owned(vec![op_execute_tool(), op_agent_llm_call()]),
        ..Default::default()
    }
}

// ── ExecutionResult + LogLine ─────────────────────────────────────────────────

/// Result of executing a framework-wrapped function.
#[derive(Debug)]
pub struct ExecutionResult {
    pub output: serde_json::Value,
    pub logs:   Vec<LogLine>,
}

/// A structured log line emitted by user code or the tool executor.
/// `span_type` and `source` allow the trace viewer to render distinct span kinds.
#[derive(Debug, serde::Deserialize)]
pub struct LogLine {
    pub level:   String,
    pub message: String,
    /// "event" (default) | "tool" | "start" | "end"
    #[serde(default)]
    pub span_type: Option<String>,
    /// "function" (default) | "tool"
    #[serde(default)]
    pub source: Option<String>,
}

pub async fn execute_function(
    code:        String,
    secrets:     HashMap<String, String>,
    payload:     serde_json::Value,
    tenant_id:   String,
    tenant_slug: String,
) -> Result<ExecutionResult, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();

    // Composio is a first-party Fluxbase service — the platform-level API key is set as
    // an env var on the runtime service (COMPOSIO_API_KEY). Users do not supply their own key.
    // Each tenant is a separate Composio entity, scoped under their tenant_id.
    let composio_api_key = std::env::var("COMPOSIO_API_KEY").ok();

    // Each tenant is a Composio "entity" — their connected accounts are scoped under this ID.
    // Allow override via COMPOSIO_ENTITY_ID (e.g., for a shared demo entity like "fluxbase-demo").
    let entity_id = std::env::var("COMPOSIO_ENTITY_ID")
        .unwrap_or_else(|_| tenant_id.clone());

    std::thread::spawn(move || {
        let tokio_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let result = tokio_rt.block_on(async move {
            // Initialize Deno with the tools + agent extension registered
            let ext = build_fluxbase_extension();
            let mut rt = JsRuntime::new(RuntimeOptions {
                extensions: vec![ext],
                ..Default::default()
            });

            // Inject tool state so op_execute_tool can read it
            rt.op_state().borrow_mut().put(ToolOpState {
                api_key:   composio_api_key,
                entity_id: entity_id.clone(),
            });

            // Inject agent LLM state so op_agent_llm_call can read it
            let llm_key   = secrets.get("FLUXBASE_LLM_KEY").cloned();
            let llm_url   = secrets
                .get("FLUXBASE_LLM_URL")
                .cloned()
                .unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());
            let llm_model = secrets
                .get("FLUXBASE_LLM_MODEL")
                .cloned()
                .unwrap_or_else(|| "gpt-4o-mini".to_string());
            rt.op_state().borrow_mut().put(AgentOpState { llm_key, llm_url, llm_model });

            let secrets_json     = serde_json::to_string(&secrets).map_err(|e| e.to_string())?;
            let payload_json     = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
            let transformed_code = code;

            let wrapper = format!(r#"
                var __fluxbase_fn;

                (async () => {{
                    const __fluxbase_logs = [];

                    const __secrets = {secrets_json};
                    const __payload = {payload_json};

                    // ── Full FluxContext implementation ────────────────────────
                    const __ctx = {{
                        tenant: {{
                            id:   "{tenant_id}",
                            slug: "{tenant_slug}",
                        }},
                        payload: __payload,
                        env:     __secrets,

                        // Secrets accessor
                        secrets: {{
                            get: (key) => __secrets[key] !== undefined ? __secrets[key] : null,
                        }},

                        // Structured logger
                        log: (message, level) => {{
                            __fluxbase_logs.push({{
                                level:     level || "info",
                                message:   String(message),
                                span_type: "event",
                                source:    "function",
                            }});
                        }},

                        // ── Tools — 900+ app integrations ─────────────────────
                        // ctx.tools.run("slack.send_message", input)
                        // Each call emits a trace span: tool:slack.send_message 45ms
                        tools: {{
                            run: async (toolName, input) => {{
                                const _start = Date.now();
                                try {{
                                    const result = await Deno.core.ops.op_execute_tool(
                                        toolName,
                                        input || {{}}
                                    );
                                    const duration = result._duration_ms || (Date.now() - _start);
                                    __fluxbase_logs.push({{
                                        level:     "info",
                                        message:   `tool:${{toolName}}  ${{duration}}ms`,
                                        span_type: "tool",
                                        source:    "tool",
                                    }});
                                    // Strip internal metadata before returning to user
                                    const {{ _tool, _duration_ms, _success, ...data }} = result;
                                    return data;
                                }} catch (e) {{
                                    const duration = Date.now() - _start;
                                    __fluxbase_logs.push({{
                                        level:     "error",
                                        message:   `tool:${{toolName}}  failed (${{duration}}ms): ${{e.message}}`,
                                        span_type: "tool",
                                        source:    "tool",
                                    }});
                                    throw new Error(`tool:${{toolName}} failed: ${{e.message}}`);
                                }}
                            }},
                        }},

                        // ── Workflow ─────────────────────────────────────────
                        // ctx.workflow.run([ {{ name: "step1", fn: async (ctx, prev) => ... }} ])
                        // ctx.workflow.parallel([ {{ name: "step1", fn: async (ctx) => ... }} ])
                        workflow: {{
                            run: async (steps, options) => {{
                                options = options || {{}};
                                const outputs = {{}};
                                for (const step of steps) {{
                                    const name = step.name || ("step_" + Object.keys(outputs).length);
                                    const _start = Date.now();
                                    try {{
                                        const result = await step.fn(__ctx, outputs);
                                        const duration = Date.now() - _start;
                                        __fluxbase_logs.push({{
                                            level:     "info",
                                            message:   "workflow:" + name + "  " + duration + "ms",
                                            span_type: "workflow_step",
                                            source:    "workflow",
                                        }});
                                        outputs[name] = result;
                                    }} catch (e) {{
                                        const duration = Date.now() - _start;
                                        __fluxbase_logs.push({{
                                            level:     "error",
                                            message:   "workflow:" + name + "  failed (" + duration + "ms): " + (e && e.message),
                                            span_type: "workflow_step",
                                            source:    "workflow",
                                        }});
                                        if (options.continueOnError) {{
                                            outputs[name] = {{ __error: e && e.message }};
                                        }} else {{
                                            throw e;
                                        }}
                                    }}
                                }}
                                return outputs;
                            }},
                            parallel: async (steps) => {{
                                const settled = await Promise.allSettled(steps.map(function(step) {{
                                    const name = step.name || "step";
                                    const _start = Date.now();
                                    return step.fn(__ctx).then(function(result) {{
                                        const duration = Date.now() - _start;
                                        __fluxbase_logs.push({{
                                            level:     "info",
                                            message:   "workflow:" + name + "  " + duration + "ms (parallel)",
                                            span_type: "workflow_step",
                                            source:    "workflow",
                                        }});
                                        return result;
                                    }});
                                }}));
                                const outputs = {{}};
                                settled.forEach(function(r, i) {{
                                    const name = (steps[i] && steps[i].name) ? steps[i].name : ("step_" + i);
                                    outputs[name] = r.status === "fulfilled" ? r.value : {{ __error: r.reason && r.reason.message }};
                                }});
                                return outputs;
                            }},
                        }},

                        // ── Agent ─────────────────────────────────────────────
                        // ctx.agent.run({{ goal: "...", tools: ["slack.send_message"], maxSteps: 5 }})
                        // The LLM decides which tool to call next; ctx.tools.run() executes it.
                        // Single execution layer is preserved: Agent -> tools.run() -> ToolExecutor
                        agent: {{
                            run: async (options) => {{
                                options = options || {{}};
                                const goal      = options.goal || "Complete the task";
                                const toolNames = options.tools || [];
                                const maxSteps  = options.maxSteps || 5;
                                const toolDefs  = toolNames.map(function(t) {{
                                    return {{
                                        type: "function",
                                        function: {{
                                            name:        t.replace(".", "_"),
                                            description: "Execute the " + t + " Fluxbase integration",
                                            parameters:  {{ type: "object", properties: {{}} }},
                                        }},
                                    }};
                                }});
                                const messages = [
                                    {{
                                        role:    "system",
                                        content: "You are a Fluxbase automation agent. Goal: " + goal + ". Available tools: " + (toolNames.length > 0 ? toolNames.join(", ") : "none") + ". Respond only with JSON. To call a tool: {{\"done\":false,\"tool\":\"tool.name\",\"input\":{{}}}}. When complete: {{\"done\":true,\"answer\":\"what was done\"}}.",
                                    }},
                                    {{ role: "user", content: "Complete this goal: " + goal }},
                                ];
                                let lastOutput = null;
                                for (let step = 0; step < maxSteps; step++) {{
                                    const _start   = Date.now();
                                    const decision = await Deno.core.ops.op_agent_llm_call(messages, toolDefs);
                                    const duration = Date.now() - _start;
                                    const label    = decision.done ? "[done]" : ("tool=" + (decision.tool || "?"));
                                    __fluxbase_logs.push({{
                                        level:     "info",
                                        message:   "agent:step=" + (step + 1) + "  " + duration + "ms  " + label,
                                        span_type: "agent_step",
                                        source:    "agent",
                                    }});
                                    if (decision.done) {{
                                        return {{ answer: decision.answer, steps: step + 1, output: lastOutput }};
                                    }}
                                    if (!decision.tool) {{
                                        throw new Error("agent: LLM returned neither done=true nor a tool name");
                                    }}
                                    lastOutput = await __ctx.tools.run(decision.tool, decision.input || {{}});
                                    messages.push({{ role: "assistant", content: JSON.stringify({{ tool: decision.tool, input: decision.input }}) }});
                                    messages.push({{ role: "user", content: "Tool " + decision.tool + " returned: " + JSON.stringify(lastOutput) }});
                                }}
                                throw new Error("agent: exceeded maxSteps=" + maxSteps);
                            }},
                        }},
                    }};

                    // Execute the bundle
                    {transformed_code}

                    let __result;
                    let target_fn = __fluxbase_fn;

                    // esbuild wraps the default export under .default
                    if (target_fn && target_fn.default) {{
                        target_fn = target_fn.default;
                    }}

                    if (typeof target_fn === 'object' && target_fn !== null && target_fn.__fluxbase === true) {{
                        try {{
                            __result = await target_fn.execute(__payload, __ctx);
                        }} catch (e) {{
                            const code = e.code || 'EXECUTION_ERROR';
                            throw new Error(JSON.stringify({{ code, message: e.message }}));
                        }}
                    }} else if (typeof target_fn === 'function') {{
                        __result = await target_fn(__ctx);
                    }} else {{
                        throw new Error("Bundle must export a defineFunction() result or an async function. Got: " + typeof target_fn);
                    }}

                    return {{ result: __result, logs: __fluxbase_logs }};
                }})()
            "#,
                secrets_json     = secrets_json,
                payload_json     = payload_json,
                transformed_code = transformed_code,
                tenant_id        = tenant_id,
                tenant_slug      = tenant_slug,
            );

            let res = timeout(Duration::from_secs(30), async {
                let res = rt.execute_script("<anon>", wrapper)
                    .map_err(|e| format!("Execution error: {}", e))?;

                let resolved_future = rt.resolve(res);
                let resolved = rt.with_event_loop_promise(resolved_future, Default::default()).await
                    .map_err(|e| format!("Promise resolution error: {}", e))?;

                let mut scope = rt.handle_scope();
                let local     = deno_core::v8::Local::new(&mut scope, resolved);

                let json_val = deno_core::serde_v8::from_v8::<serde_json::Value>(&mut scope, local)
                    .map_err(|e| format!("Serialization error: {}", e))?;

                Ok(json_val)
            }).await;

            match res {
                Ok(Ok(val)) => {
                    let output = val.get("result").cloned().unwrap_or(val.clone());
                    let logs: Vec<LogLine> = val.get("logs")
                        .and_then(|l| serde_json::from_value(l.clone()).ok())
                        .unwrap_or_default();
                    Ok(ExecutionResult { output, logs })
                }
                Ok(Err(e)) => Err(e),
                Err(_)     => Err("Function execution timed out after 30 seconds".to_string()),
            }
        });

        let _ = tx.send(result);
    });

    match timeout(Duration::from_secs(32), rx).await {
        Ok(Ok(val)) => val,
        Ok(Err(_))  => Err("Thread execution channel dropped".to_string()),
        Err(_)      => Err("Thread execution timed out".to_string()),
    }
}
