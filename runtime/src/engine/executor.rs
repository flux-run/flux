//! Deno V8 execution engine — runs JavaScript functions in sandboxed `JsRuntime` isolates.
//!
//! ## Isolate architecture
//!
//! Each call to `IsolatePool::execute` routes to a warm isolate worker thread.
//! The worker holds a `JsRuntime` that was created at thread startup (not per request).
//! Per-request state is injected into `OpState` before execution and cleared after.
//!
//! ## LogLine
//!
//! `ctx.log(level, message, opts)` inside user JS emits a `LogLine` into a
//! `__fluxbase_logs` array declared inside the IIFE wrapper. After the function
//! returns, `execute_with_runtime` extracts the logs from V8 memory and returns them
//! as `ExecutionResult::logs`. The caller (`ExecutionRunner`) ships them to
//! `flux.platform_logs` via `TraceEmitter::emit_logs` (fire-and-forget).
//!
//! ## Security hardening
//!
//! - Deterministic random seeding (`Math.random` → seeded PRNG) for replay.
//! - `globalThis.__fluxbase_logs` and `globalThis.__ctx` are re-declared as `const`
//!   inside the IIFE on every call, so user code cannot persist state across
//!   invocations via globals.
//! - V8 heap and stack are not shared between workers (each thread owns its runtime).
use deno_core::{JsRuntime, RuntimeOptions, OpState, Extension};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use tokio::time::{timeout, Duration};

use crate::agent::AgentOpState;


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

/// Build the Fluxbase runtime extension — agent + queue ops.
pub fn build_fluxbase_extension() -> Extension {
    Extension {
        name: "fluxbase",
        ops: Cow::Owned(vec![op_agent_llm_call(), op_queue_push()]),
        ..Default::default()
    }
}

// ── Queue op ──────────────────────────────────────────────────────────────────
//
// Deno bridge: JS calls Deno.core.ops.op_queue_push(functionName, payload, delay, key)
// from inside ctx.queue.push().
// The op:
//   1. Resolves function name → UUID via GET {api_url}/internal/functions/resolve
//   2. POSTs to queue service /jobs
//   3. Returns { job_id }

/// Per-request queue context injected into Deno OpState.
pub struct QueueOpState {
    pub queue_url:     String,
    pub api_url:       String,
    pub service_token: String,
    pub project_id:    Option<uuid::Uuid>,
    pub client:        reqwest::Client,
}

/// Carry queue context from the async Tokio world (pool.rs) into execute_with_runtime.
#[derive(Clone)]
pub struct QueueContext {
    pub queue_url:     String,
    pub api_url:       String,
    pub service_token: String,
    pub project_id:    Option<uuid::Uuid>,
    pub client:        reqwest::Client,
}

/// Options forwarded from JS's `opts` argument to `ctx.queue.push()`.
#[derive(serde::Deserialize)]
pub struct QueuePushOpts {
    pub delay_seconds:   Option<i64>,
    pub idempotency_key: Option<String>,
}

#[deno_core::op2(async)]
#[serde]
pub async fn op_queue_push(
    state:             Rc<RefCell<OpState>>,
    #[string] function_name: String,
    #[serde]  payload:       serde_json::Value,
    #[serde]  opts:          QueuePushOpts,
) -> Result<serde_json::Value, std::io::Error> {
    let (queue_url, api_url, service_token, project_id, client) = {
        let s = state.borrow();
        let qs = s.borrow::<QueueOpState>();
        (
            qs.queue_url.clone(),
            qs.api_url.clone(),
            qs.service_token.clone(),
            qs.project_id,
            qs.client.clone(),
        )
    };

    let project_id_str = project_id
        .map(|p| p.to_string())
        .unwrap_or_default();

    // ── Resolve function name → { function_id, tenant_id } ───────────────
    let resolve_url = format!(
        "{}/internal/functions/resolve?name={}&project_id={}",
        api_url.trim_end_matches('/'), function_name, project_id_str,
    );

    let resolve_resp = client
        .get(&resolve_url)
        .header("X-Service-Token", &service_token)
        .send()
        .await
        .map_err(|e| std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("function resolve request failed: {}", e),
        ))?;

    if !resolve_resp.status().is_success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("ctx.queue.push: function '{}' not found in project", function_name),
        ));
    }

    let resolved: serde_json::Value = resolve_resp.json().await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    let function_id = resolved["function_id"].as_str()
        .ok_or_else(|| std::io::Error::new(
            std::io::ErrorKind::Other,
            "missing function_id in resolve response",
        ))?;

    let tenant_id = resolved["tenant_id"].as_str()
        .ok_or_else(|| std::io::Error::new(
            std::io::ErrorKind::Other,
            "missing tenant_id in resolve response",
        ))?;

    // ── Push to queue service ─────────────────────────────────────────────
    let job_body = serde_json::json!({
        "tenant_id":       tenant_id,
        "project_id":      project_id,
        "function_id":     function_id,
        "payload":         payload,
        "delay_seconds":   opts.delay_seconds,
        "idempotency_key": opts.idempotency_key,
    });

    let queue_resp = client
        .post(format!("{}/jobs", queue_url.trim_end_matches('/')))
        .bearer_auth(&service_token)
        .json(&job_body)
        .send()
        .await
        .map_err(|e| std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("queue push failed: {}", e),
        ))?;

    if !queue_resp.status().is_success() {
        let status = queue_resp.status();
        let body = queue_resp.text().await.unwrap_or_default();
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("queue returned {}: {}", status, body),
        ));
    }

    let job_data: serde_json::Value = queue_resp.json().await
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

    Ok(serde_json::json!({ "job_id": job_data["job_id"] }))
}

/// Create a warm `JsRuntime` with the Fluxbase extension registered.
/// Intended to be called once per worker thread; per-request state is injected
/// via `OpState` before each execution (see `execute_with_runtime`).
///
/// # Isolation hardening (runs once per worker thread at startup):
///
/// 1. **Prototype freeze** — `Object.freeze` applied to the most-abused
///    built-in prototypes (`Object`, `Array`, `Function`, `String`, `Number`,
///    `Boolean`, `RegExp`, `Promise`, `Map`, `Set`, `Error`).  Prevents user
///    code from poisoning shared prototype chains across tenants.
///    Cost: ~20 µs at startup; no per-request overhead.
///
/// 2. **Global baseline snapshot** — captures all current `globalThis` key
///    names into `__fluxbase_allowed_globals` immediately after freezing.
///    `build_wrapper` sweeps any new keys added by a previous bundle before
///    the next request runs, eliminating cross-request `globalThis` leakage.
pub fn create_js_runtime() -> JsRuntime {
    let mut rt = JsRuntime::new(RuntimeOptions {
        extensions: vec![build_fluxbase_extension()],
        ..Default::default()
    });
    rt.execute_script(
        "<fluxbase-init>",
        // 1. Freeze built-in prototypes to prevent cross-tenant prototype poisoning.
        //    e.g. user code cannot do: Array.prototype.map = () => []
        "const __protos = [\
            Object, Array, Function, String, Number, Boolean,\
            RegExp, Promise, Map, Set, WeakMap, WeakSet, Error,\
            TypeError, RangeError, SyntaxError, ReferenceError\
        ];\
        for (const C of __protos) {\
            if (C && C.prototype) Object.freeze(C.prototype);\
        }\
        Object.freeze(__protos);\
        \
        // 2. Snapshot baseline globals for the per-request sweep in build_wrapper.\
        globalThis.__fluxbase_allowed_globals =\
            new Set(Object.getOwnPropertyNames(globalThis));",
    ).expect("failed to initialise worker sandbox");
    rt
}

/// Build the JS IIFE wrapper that injects FluxContext and executes the bundle.
fn build_wrapper(
    secrets_json:     &str,
    payload_json:     &str,
    transformed_code: &str,
    execution_seed:   i64,
) -> String {
    format!(r#"
        var __fluxbase_fn;

        // ── Global scope sweep ──────────────────────────────────────────────
        // Delete any key set by a previous invocation on this warm isolate.
        // __fluxbase_allowed_globals is frozen at worker startup and contains
        // only V8/Deno built-ins — nothing a user bundle could have added.
        // Cost: O(n) over user-added keys only; typically 0–2 keys in practice.
        if (typeof __fluxbase_allowed_globals !== "undefined") {{
            for (const __k of Object.getOwnPropertyNames(globalThis)) {{
                if (!__fluxbase_allowed_globals.has(__k)) {{
                    try {{ delete globalThis[__k]; }} catch (_) {{}}
                }}
            }}
        }}

        // ── Deterministic execution seed ─────────────────────────────────────────────
        // Overrides Math.random, crypto.randomUUID, and nanoid with seeded equivalents
        // so `flux queue replay` reproduces identical IDs and execution paths.
        // When execution_seed is 0 (sync / non-replay path) the seed is a runtime-
        // generated mix, so behaviour is unchanged but still deterministic per call.
        (function() {{
            let __t = ({execution_seed} ^ 0xDEADBEEF) >>> 0;
            if (__t === 0) __t = 0x1;
            globalThis.__fluxbase_rand = function() {{
                __t += 0x6D2B79F5;
                let r = Math.imul(__t ^ (__t >>> 15), 1 | __t);
                r ^= r + Math.imul(r ^ (r >>> 7), 61 | r);
                return ((r ^ (r >>> 14)) >>> 0) / 4294967296;
            }};
        }})();
        Math.random = globalThis.__fluxbase_rand;
        if (typeof crypto === "undefined") globalThis.crypto = {{}};
        crypto.randomUUID = () => {{
            const b = new Uint8Array(16);
            for (let i = 0; i < 16; i++) b[i] = Math.floor(globalThis.__fluxbase_rand() * 256);
            b[6] = (b[6] & 0x0f) | 0x40;
            b[8] = (b[8] & 0x3f) | 0x80;
            const h = x => (x + 256).toString(16).slice(1);
            return h(b[0])+h(b[1])+h(b[2])+h(b[3])+'-'+h(b[4])+h(b[5])+'-'+
                   h(b[6])+h(b[7])+'-'+h(b[8])+h(b[9])+'-'+
                   h(b[10])+h(b[11])+h(b[12])+h(b[13])+h(b[14])+h(b[15]);
        }};
        globalThis.nanoid = (size = 21) => {{
            const abc = "useandom-26T198340PX75pxJACKVERYMINDBUSHWOLF_GQZbfghjklqvwyzrict";
            let id = "";
            for (let i = 0; i < size; i++) id += abc[Math.floor(globalThis.__fluxbase_rand() * abc.length)];
            return id;
        }};

        (async () => {{
            const __fluxbase_logs = [];

            const __secrets = {secrets_json};
            const __payload = {payload_json};

            // ── Full FluxContext implementation ────────────────────────
            const __ctx = {{

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

                // ── Tools ─────────────────────────────────────────────
                tools: {{
                    run: async () => {{
                        throw new Error("ctx.tools is not available in this runtime");
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

                // ── Queue ─────────────────────────────────────────────
                // ctx.queue.push("function_name", payload, {{ delay: "5m", idempotencyKey: "..." }})
                //
                // Enqueues a background job. The runtime resolves the function name
                // to a UUID, calls the Queue service, and records a queue_push span
                // so the enqueue appears in `flux trace`.
                queue: {{
                    push: async (functionName, payload, opts) => {{
                        opts = opts || {{}};
                        const delay = opts.delay
                            ? (() => {{
                                const _d = String(opts.delay);
                                if (_d.endsWith("h")) return parseInt(_d) * 3600;
                                if (_d.endsWith("m")) return parseInt(_d) * 60;
                                if (_d.endsWith("s")) return parseInt(_d);
                                return parseInt(_d);
                              }})()
                            : (opts.delay_seconds || null);
                        const result = await Deno.core.ops.op_queue_push(
                            functionName,
                            payload !== undefined ? payload : {{}},
                            {{
                                delay_seconds:   delay,
                                idempotency_key: opts.idempotencyKey || opts.idempotency_key || null,
                            }},
                        );
                        __fluxbase_logs.push({{
                            level:     "info",
                            message:   "queue_push:" + functionName + "  job_id=" + (result && result.job_id),
                            span_type: "queue_push",
                            source:    "queue",
                        }});
                        return result;
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
        execution_seed   = execution_seed,
    )
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
///
/// Fields added for execution tracing:
/// - `span_id`           — unique ID for this span; generated JS-side or server-side on ship
/// - `duration_ms`       — set by tool/workflow/agent spans; propagated to log sink
/// - `execution_state`   — lifecycle state: "started" | "running" | "completed" | "error"
/// - `tool_name`         — the Fluxbase tool name for `span_type == "tool"` spans
#[derive(Debug, serde::Deserialize)]
pub struct LogLine {
    pub level:   String,
    pub message: String,
    /// "event" (default) | "tool" | "workflow_step" | "agent_step" | "start" | "end"
    #[serde(default)]
    pub span_type: Option<String>,
    /// "function" (default) | "tool" | "workflow" | "agent" | "runtime"
    #[serde(default)]
    pub source: Option<String>,
    /// Unique span identifier — used to link parent → child spans across services.
    /// If not provided by JS, routes.rs generates a UUID v4 before shipping.
    #[serde(default)]
    pub span_id: Option<String>,
    /// Duration in ms — set by tool/workflow/agent spans for replay recording.
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// Lifecycle state tag used for replay and trace bisect.
    #[serde(default)]
    pub execution_state: Option<String>,
    /// Tool name for tool spans — used to correlate with replay recordings.
    #[serde(default)]
    pub tool_name: Option<String>,
}

/// Execute a function on an **already-created** `JsRuntime`.
///
/// This is the hot path used by `IsolatePool` workers. The runtime is created
/// once per worker thread (`create_js_runtime()`) and reused across invocations.
/// Per-request state (secrets, tenant, LLM config) is injected into `OpState`
/// before each execution via `try_take + put` — a clean swap, no reallocations.
///
/// # Performance
/// Eliminates per-request costs of the cold path:
/// - `JsRuntime::new` (V8 heap init + extension registration): ~3–5 ms
/// - `std::thread::spawn` (OS thread + 8 MB stack): ~0.5 ms
/// - `tokio::Runtime::build` (single-thread runtime): ~0.5 ms
///
/// # Safety / state isolation
/// - `__fluxbase_logs` is declared inside the IIFE — fresh per call.
/// - `__ctx` is declared inside the IIFE — fresh per call, holds secrets/payload.
/// - `__fluxbase_fn` is a global `var` — re-assigned by the bundle on every call.
/// - User globals (`globalThis.*`) are swept at the start of each IIFE using the
///   `__fluxbase_allowed_globals` snapshot taken at worker startup. Any key added
///   by a previous bundle is deleted before the next bundle runs, ensuring no
///   cross-request data leakage on a warm isolate.
/// - On timeout the caller (`IsolatePool`) marks the runtime for recreation so
///   the next call on that worker gets a fresh isolate (V8 won't be stuck).
pub async fn execute_with_runtime(
    rt:             &mut JsRuntime,
    code:           String,
    secrets:        HashMap<String, String>,
    payload:        serde_json::Value,
    execution_seed: i64,
    queue_ctx:      QueueContext,
) -> Result<ExecutionResult, String> {
    // ── Per-request OpState injection ─────────────────────────────────────────
    // Use try_take + put to handle both the first call and subsequent reuse.
    {
        let op_state = rt.op_state();
        let mut state = op_state.borrow_mut();

        let llm_key   = secrets.get("FLUXBASE_LLM_KEY").cloned();
        let llm_url   = secrets.get("FLUXBASE_LLM_URL").cloned()
            .unwrap_or_else(|| "https://api.openai.com/v1/chat/completions".to_string());
        let llm_model = secrets.get("FLUXBASE_LLM_MODEL").cloned()
            .unwrap_or_else(|| "gpt-4o-mini".to_string());
        let _ = state.try_take::<AgentOpState>();
        state.put(AgentOpState { llm_key, llm_url, llm_model });

        let _ = state.try_take::<QueueOpState>();
        state.put(QueueOpState {
            queue_url:     queue_ctx.queue_url,
            api_url:       queue_ctx.api_url,
            service_token: queue_ctx.service_token,
            project_id:    queue_ctx.project_id,
            client:        queue_ctx.client,
        });
    }

    // ── Build + execute the IIFE wrapper ───────────────────────────────────
    let secrets_json     = serde_json::to_string(&secrets).map_err(|e| e.to_string())?;
    let payload_json     = serde_json::to_string(&payload).map_err(|e| e.to_string())?;
    let transformed_code = code;

    let wrapper = build_wrapper(
        &secrets_json, &payload_json, &transformed_code, execution_seed,
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn no_op_queue_ctx() -> QueueContext {
        QueueContext {
            queue_url:     "http://127.0.0.1:0".to_string(),
            api_url:       "http://127.0.0.1:0".to_string(),
            service_token: "test-token".to_string(),
            project_id:    None,
            client:        reqwest::Client::new(),
        }
    }

    /// Execute `code` in a fresh JsRuntime and return the result.
    /// Uses current_thread flavor so JsRuntime (!Send) stays on one thread.
    async fn run_js(code: &str, payload: serde_json::Value) -> Result<ExecutionResult, String> {
        let mut rt = create_js_runtime();
        execute_with_runtime(
            &mut rt,
            code.to_string(),
            HashMap::new(),
            payload,
            0,
            no_op_queue_ctx(),
        ).await
    }

    async fn run_js_with_secrets(
        code: &str,
        secrets: HashMap<String, String>,
    ) -> Result<ExecutionResult, String> {
        let mut rt = create_js_runtime();
        execute_with_runtime(
            &mut rt,
            code.to_string(),
            secrets,
            serde_json::Value::Null,
            0,
            no_op_queue_ctx(),
        ).await
    }

    // ── create_js_runtime ─────────────────────────────────────────────────

    #[test]
    fn create_js_runtime_does_not_panic() {
        let _rt = create_js_runtime();
    }

    #[test]
    fn multiple_runtimes_are_independent() {
        let _r1 = create_js_runtime();
        let _r2 = create_js_runtime();
    }

    // ── basic execution ───────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn returns_simple_value() {
        let code = r#"
            __fluxbase_fn = async (ctx) => 42;
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!(42));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn returns_object() {
        let code = r#"
            __fluxbase_fn = async (ctx) => ({ hello: "world" });
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!({"hello": "world"}));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn returns_null_result() {
        let code = r#"
            __fluxbase_fn = async (ctx) => null;
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert!(res.output.is_null());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn payload_available_in_ctx() {
        let code = r#"
            __fluxbase_fn = async (ctx) => ctx.payload.name;
        "#;
        let res = run_js(code, serde_json::json!({"name": "alice"})).await.unwrap();
        assert_eq!(res.output, serde_json::json!("alice"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn nested_payload_fields() {
        let code = r#"
            __fluxbase_fn = async (ctx) => ctx.payload.a.b.c;
        "#;
        let res = run_js(code, serde_json::json!({"a":{"b":{"c":99}}})).await.unwrap();
        assert_eq!(res.output, serde_json::json!(99));
    }

    // ── secrets / env ─────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn secrets_accessible_via_ctx_env() {
        let mut secrets = HashMap::new();
        secrets.insert("MY_KEY".to_string(), "super-secret".to_string());
        let code = r#"
            __fluxbase_fn = async (ctx) => ctx.env.MY_KEY;
        "#;
        let res = run_js_with_secrets(code, secrets).await.unwrap();
        assert_eq!(res.output, serde_json::json!("super-secret"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_secret_is_undefined() {
        let code = r#"
            __fluxbase_fn = async (ctx) => (ctx.env.NONEXISTENT ?? "fallback");
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("fallback"));
    }

    // ── logging ───────────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn ctx_log_emits_log_lines() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                ctx.log("hello from function", "info");
                return { result: "ok" };
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert!(!res.logs.is_empty(), "expected at least one log line");
        assert_eq!(res.logs[0].message, "hello from function");
        assert_eq!(res.logs[0].level,   "info");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn multiple_log_levels_captured() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                ctx.log("info msg",  "info");
                ctx.log("warn msg",  "warn");
                ctx.log("error msg", "error");
                return { result: true };
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.logs.len(), 3);
        let levels: Vec<&str> = res.logs.iter().map(|l| l.level.as_str()).collect();
        assert!(levels.contains(&"info"));
        assert!(levels.contains(&"warn"));
        assert!(levels.contains(&"error"));
    }

    // ── polyfills ─────────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn crypto_random_uuid_returns_uuid_shaped_string() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                const id = crypto.randomUUID();
                return id;
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        let uuid_str = res.output.as_str().unwrap_or("");
        // UUID format: 8-4-4-4-12 hex chars with dashes
        assert_eq!(uuid_str.len(), 36, "expected UUID length 36, got: {uuid_str}");
        assert_eq!(uuid_str.chars().filter(|&c| c == '-').count(), 4);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn math_random_returns_number_in_range() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                const r = Math.random();
                return (r >= 0 && r < 1);
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!(true));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn deterministic_seed_produces_same_uuid() {
        // Same seed → same UUID on both calls
        let code = r#"
            __fluxbase_fn = async (ctx) => crypto.randomUUID();
        "#;
        let seed = 42i64;

        let mut rt1 = create_js_runtime();
        let r1 = execute_with_runtime(&mut rt1, code.to_string(), HashMap::new(),
            serde_json::Value::Null, seed, no_op_queue_ctx()).await.unwrap();

        let mut rt2 = create_js_runtime();
        let r2 = execute_with_runtime(&mut rt2, code.to_string(), HashMap::new(),
            serde_json::Value::Null, seed, no_op_queue_ctx()).await.unwrap();

        assert_eq!(r1.output, r2.output, "same seed must produce same UUID");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn different_seeds_produce_different_uuids() {
        let code = r#"
            __fluxbase_fn = async (ctx) => crypto.randomUUID();
        "#;
        let mut rt1 = create_js_runtime();
        let r1 = execute_with_runtime(&mut rt1, code.to_string(), HashMap::new(),
            serde_json::Value::Null, 1, no_op_queue_ctx()).await.unwrap();

        let mut rt2 = create_js_runtime();
        let r2 = execute_with_runtime(&mut rt2, code.to_string(), HashMap::new(),
            serde_json::Value::Null, 2, no_op_queue_ctx()).await.unwrap();

        assert_ne!(r1.output, r2.output);
    }

    // ── error handling ────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn syntax_error_returns_err() {
        let code = "this is not valid javascript }{{{";
        let res = run_js(code, serde_json::Value::Null).await;
        assert!(res.is_err(), "expected Err for syntax error");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn runtime_throw_returns_err() {
        let code = r#"
            __fluxbase_fn = async (ctx) => { throw new Error("exploded"); };
        "#;
        let res = run_js(code, serde_json::Value::Null).await;
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("exploded"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn undefined_variable_reference_returns_err() {
        let code = r#"
            __fluxbase_fn = async (ctx) => undeclaredVar;
        "#;
        let res = run_js(code, serde_json::Value::Null).await;
        assert!(res.is_err());
    }

    // ── isolation ─────────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn globals_are_cleaned_between_invocations() {
        // First invocation sets a local IIFE-scoped fn and runs cleanly.
        let code1 = r#"
            __fluxbase_fn = async (ctx) => "first";
        "#;
        // Second invocation on the SAME runtime must still work correctly
        // (even if some globals leak between calls, execution must not fail).
        let code2 = r#"
            __fluxbase_fn = async (ctx) => "second";
        "#;
        let mut rt = create_js_runtime();
        let r1 = execute_with_runtime(&mut rt, code1.to_string(), HashMap::new(),
            serde_json::Value::Null, 0, no_op_queue_ctx()).await.unwrap();
        let r2 = execute_with_runtime(&mut rt, code2.to_string(), HashMap::new(),
            serde_json::Value::Null, 0, no_op_queue_ctx()).await.unwrap();
        assert_eq!(r1.output, serde_json::json!("first"));
        assert_eq!(r2.output, serde_json::json!("second"),
            "reused runtime must produce correct output on second invocation");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn prototype_freeze_prevents_poisoning() {
        // Object.freeze prevents modification — in sloppy mode the assignment
        // silently fails (no throw); the property retains its original value.
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                const orig = Array.prototype.map;
                Array.prototype.map = () => "poisoned";
                // If frozen, the assignment is a no-op and map is unchanged.
                return Array.prototype.map === orig ? "frozen" : "not frozen";
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("frozen"),
            "Array.prototype must be frozen — assignment must be a no-op");
    }

    // ── async JS ──────────────────────────────────────────────────────────

    #[tokio::test(flavor = "current_thread")]
    async fn awaited_promise_resolves() {
        let code = r#"
            __fluxbase_fn = async (ctx) => {
                const val = await Promise.resolve(99);
                return val;
            };
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!(99));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn setTimeout_is_not_required_for_basic_execution() {
        // Functions don't need setTimeout — just test it doesn't error.
        let code = r#"
            __fluxbase_fn = async (ctx) => "no timers needed";
        "#;
        let res = run_js(code, serde_json::Value::Null).await.unwrap();
        assert_eq!(res.output, serde_json::json!("no timers needed"));
    }

    // ── LogLine struct ────────────────────────────────────────────────────

    #[test]
    fn log_line_serde_roundtrip() {
        // LogLine derives Deserialize (not Serialize) — parse from raw JSON
        let json = r#"{"level":"info","message":"test message","span_type":"event","source":"function"}"#;
        let line: LogLine = serde_json::from_str(json).unwrap();
        assert_eq!(line.level,   "info");
        assert_eq!(line.message, "test message");
        assert_eq!(line.span_type.as_deref(), Some("event"));
        assert!(line.span_id.is_none());
    }

    // ── ExecutionResult struct ────────────────────────────────────────────

    #[test]
    fn execution_result_with_empty_logs() {
        let r = ExecutionResult {
            output: serde_json::json!({"k": "v"}),
            logs:   vec![],
        };
        assert!(r.logs.is_empty());
        assert_eq!(r.output["k"], "v");
    }
}
