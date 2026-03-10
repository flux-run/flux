use deno_core::{JsRuntime, RuntimeOptions, OpState, Extension};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use tokio::time::{timeout, Duration};

use crate::tools::executor::ToolOpState;
use crate::tools::registry::ToolRegistry;
use crate::tools::composio;

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
            "tools_not_configured: FLUXBASE_COMPOSIO_KEY secret not set",
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

/// Build the Fluxbase tools extension manually (avoids extension! macro API drift).
fn build_tools_extension() -> Extension {
    Extension {
        name: "fluxbase_tools",
        ops: Cow::Owned(vec![op_execute_tool()]),
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

    // Resolve Composio API key from tenant secrets.
    // Users set this via: flux secrets set FLUXBASE_COMPOSIO_KEY <key>
    let composio_api_key = secrets.get("FLUXBASE_COMPOSIO_KEY")
        .or_else(|| secrets.get("COMPOSIO_API_KEY"))
        .cloned();

    // Each tenant is a Composio "entity" — their connected accounts are scoped under this ID.
    let entity_id = tenant_id.clone();

    std::thread::spawn(move || {
        let tokio_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let result = tokio_rt.block_on(async move {
            // Initialize Deno with the tools extension registered
            let ext = build_tools_extension();
            let mut rt = JsRuntime::new(RuntimeOptions {
                extensions: vec![ext],
                ..Default::default()
            });

            // Inject tool state so op_execute_tool can read it
            rt.op_state().borrow_mut().put(ToolOpState {
                api_key:   composio_api_key,
                entity_id: entity_id.clone(),
            });

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
