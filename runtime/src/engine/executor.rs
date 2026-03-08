use deno_core::{JsRuntime, RuntimeOptions};
use std::collections::HashMap;
use tokio::time::{timeout, Duration};

/// Result of executing a framework-wrapped function.
/// The runtime wrapper returns both the function result and any log lines.
#[derive(Debug)]
pub struct ExecutionResult {
    pub output: serde_json::Value,
    pub logs: Vec<LogLine>,
}

#[derive(Debug, serde::Deserialize)]
pub struct LogLine {
    pub level: String,
    pub message: String,
}

pub async fn execute_function(
    code: String,
    secrets: HashMap<String, String>,
    payload: serde_json::Value,
) -> Result<ExecutionResult, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();

    std::thread::spawn(move || {
        let tokio_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let result = tokio_rt.block_on(async move {
            let mut rt = JsRuntime::new(RuntimeOptions {
                ..Default::default()
            });

            let secrets_json = serde_json::to_string(&secrets).map_err(|e| e.to_string())?;
            let payload_json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;

            // The CLI builds bundles using --format=iife --global-name=__fluxbase_fn
            // So `code` will declare `var __fluxbase_fn = (() => { ... })();`
            // We just need to execute it and then use `__fluxbase_fn`.
            // Some older or raw bundles might not use this variable, but our framework does.
            let transformed_code = code;

            // Build the runtime wrapper that:
            // 1. Provides a full context object matching FluxContext interface
            // 2. Evaluates the bundle code (which sets __fluxbase_fn)
            // 3. Calls __fluxbase_fn.default.execute() (the defineFunction contract)
            // 4. Collects ctx.log() calls into __fluxbase_logs
            let wrapper = format!(r#"
                // Give the bundle a place to define its global
                var __fluxbase_fn;

                (async () => {{
                    const __fluxbase_logs = [];

                    const __secrets = {secrets_json};
                    const __payload = {payload_json};

                    // Full FluxContext implementation
                    const __ctx = {{
                        payload: __payload,
                        env: __secrets,
                        secrets: {{
                            get: (key) => __secrets[key] !== undefined ? __secrets[key] : null,
                        }},
                        log: (message, level) => {{
                            __fluxbase_logs.push({{
                                level: level || "info",
                                message: String(message),
                            }});
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
                        // Framework-wrapped function (defineFunction) — use execute() contract
                        try {{
                            __fluxbase_logs.push({{ level: "debug", message: "Calling target_fn.execute()" }});
                            __result = await target_fn.execute(__payload, __ctx);
                            __fluxbase_logs.push({{ level: "debug", message: "target_fn.execute() returned smoothly" }});
                        }} catch (e) {{
                            const code = e.code || 'EXECUTION_ERROR';
                            throw new Error(JSON.stringify({{ code, message: e.message }}));
                        }}
                    }} else if (typeof target_fn === 'function') {{
                        // Legacy raw function — call directly with ctx
                        __fluxbase_logs.push({{ level: "debug", message: "Calling raw function" }});
                        __result = await target_fn(__ctx);
                    }} else {{
                        throw new Error("Bundle must export a defineFunction() result or an async function. target_fn=" + typeof target_fn);
                    }}

                    __fluxbase_logs.push({{ level: "debug", message: "Returning result envelope" }});
                    return {{ result: __result, logs: __fluxbase_logs }};
                }})()
            "#,
                secrets_json = secrets_json,
                payload_json = payload_json,
                transformed_code = transformed_code,
            );

            let res = timeout(Duration::from_secs(10), async {
                let res = rt.execute_script("<anon>", wrapper)
                    .map_err(|e| format!("Execution error: {}", e))?;

                // Drive the event loop while waiting for our specific promise to resolve.
                let resolved_future = rt.resolve(res);
                let resolved = rt.with_event_loop_promise(resolved_future, Default::default()).await
                    .map_err(|e| format!("Promise resolution error: {}", e))?;

                let mut scope = rt.handle_scope();
                let local = deno_core::v8::Local::new(&mut scope, resolved);

                let json_val = deno_core::serde_v8::from_v8::<serde_json::Value>(&mut scope, local)
                    .map_err(|e| format!("Serialization error: {}", e))?;

                Ok(json_val)
            }).await;

            match res {
                Ok(Ok(val)) => {
                    // Extract envelope: { result, logs }
                    let output = val.get("result")
                        .cloned()
                        .unwrap_or(val.clone());
                    let logs: Vec<LogLine> = val.get("logs")
                        .and_then(|l| serde_json::from_value(l.clone()).ok())
                        .unwrap_or_default();
                    Ok(ExecutionResult { output, logs })
                }
                Ok(Err(e)) => Err(e),
                Err(_) => Err("Function execution timed out after 10 seconds".to_string()),
            }
        });

        let _ = tx.send(result);
    });

    match timeout(Duration::from_secs(12), rx).await {
        Ok(Ok(val)) => val,
        Ok(Err(_)) => Err("Thread execution channel dropped".to_string()),
        Err(_) => Err("Thread execution timed out".to_string()),
    }
}
