use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use anyhow::{Context, Result, bail};
use deno_core::error::AnyError;
use deno_core::{JsRuntime, OpState, RuntimeOptions, op2};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::isolate_pool::ExecutionContext;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionMode {
    Live,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchCheckpoint {
    pub call_index: u32,
    pub boundary: String,
    pub url: String,
    pub method: String,
    pub request: serde_json::Value,
    pub response: serde_json::Value,
    pub duration_ms: i32,
}

#[derive(Debug, Clone)]
pub struct JsExecutionOutput {
    pub output: serde_json::Value,
    pub checkpoints: Vec<FetchCheckpoint>,
}

#[derive(Debug, Clone)]
struct RuntimeExecutionState {
    context: ExecutionContext,
    call_index: u32,
    checkpoints: Vec<FetchCheckpoint>,
}

deno_core::extension!(flux_runtime_ext, ops = [op_fetch]);

#[op2(async)]
#[serde]
async fn op_fetch(
    state: Rc<RefCell<OpState>>,
    #[string] url: String,
    #[string] method: String,
    #[serde] body: Option<serde_json::Value>,
    #[serde] headers: Option<serde_json::Value>,
) -> Result<serde_json::Value, AnyError> {
    let (request_id, call_index, mode) = {
        let mut state_ref = state.borrow_mut();
        let execution = state_ref.borrow_mut::<RuntimeExecutionState>();
        let index = execution.call_index;
        execution.call_index = execution.call_index.saturating_add(1);
        (
            execution.context.request_id.clone(),
            index,
            execution.context.mode.clone(),
        )
    };

    match mode {
        ExecutionMode::Live => {
            let request_json = serde_json::json!({
                "url": url,
                "method": method,
                "body": body,
                "headers": headers,
            });

            let started = std::time::Instant::now();
            let response = make_http_request(&url, &method, body, headers).await?;
            let duration_ms = started.elapsed().as_millis() as i32;

            {
                let mut state_ref = state.borrow_mut();
                let execution = state_ref.borrow_mut::<RuntimeExecutionState>();
                execution.checkpoints.push(FetchCheckpoint {
                    call_index,
                    boundary: "http".to_string(),
                    url: url.clone(),
                    method: method.clone(),
                    request: request_json,
                    response: response.clone(),
                    duration_ms,
                });
            }

            tracing::debug!(%request_id, %call_index, %url, "intercepted fetch");
            Ok(response)
        }
    }
}

async fn make_http_request(
    url: &str,
    method: &str,
    body: Option<serde_json::Value>,
    headers: Option<serde_json::Value>,
) -> Result<serde_json::Value, AnyError> {
    let client = Client::new();
    let method = method
        .parse::<reqwest::Method>()
        .map_err(|err| deno_core::error::custom_error("TypeError", err.to_string()))?;

    let mut request = client.request(method, url);

    if let Some(raw_headers) = headers {
        let map: HashMap<String, String> = serde_json::from_value(raw_headers)
            .map_err(|err| deno_core::error::custom_error("TypeError", err.to_string()))?;
        for (key, value) in map {
            request = request.header(key, value);
        }
    }

    if let Some(body) = body {
        request = request.json(&body);
    }

    let response = request.send().await.map_err(|err| {
        deno_core::error::custom_error("TypeError", format!("fetch failed: {err}"))
    })?;

    let status = response.status().as_u16();
    let response_headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            let value = v.to_str().unwrap_or_default().to_string();
            (k.to_string(), value)
        })
        .collect::<HashMap<_, _>>();

    let text = response
        .text()
        .await
        .map_err(|err| deno_core::error::custom_error("TypeError", err.to_string()))?;

    let parsed_body = serde_json::from_str::<serde_json::Value>(&text)
        .unwrap_or_else(|_| serde_json::Value::String(text));

    Ok(serde_json::json!({
        "status": status,
        "headers": response_headers,
        "body": parsed_body,
    }))
}

pub struct JsIsolate {
    runtime: JsRuntime,
}

impl JsIsolate {
    pub fn new(user_code: &str, _isolate_id: usize) -> Result<Self> {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![flux_runtime_ext::init_ops_and_esm()],
            ..Default::default()
        });

        runtime
            .execute_script("flux:bootstrap_fetch", bootstrap_fetch_js())
            .context("failed to install fetch interceptor")?;

        let prepared = prepare_user_code(user_code);
        runtime
            .execute_script("flux:user_code", prepared)
            .context("failed to load user code")?;

        Ok(Self { runtime })
    }

    pub async fn execute(
        &mut self,
        payload: serde_json::Value,
        context: ExecutionContext,
    ) -> Result<JsExecutionOutput> {
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            state.put(RuntimeExecutionState {
                context,
                call_index: 0,
                checkpoints: Vec::new(),
            });
        }

        let payload_json = serde_json::to_string(&payload).context("failed to encode payload")?;
        let invoke = format!(
            "globalThis.__flux_last_result = null;\n\
             globalThis.__flux_last_error = null;\n\
             (async () => {{\n\
               try {{\n\
                 const result = await globalThis.__flux_user_handler({payload});\n\
                 globalThis.__flux_last_result = result ?? null;\n\
               }} catch (err) {{\n\
                 globalThis.__flux_last_error = String(err && err.stack ? err.stack : err);\n\
               }}\n\
             }})();",
            payload = payload_json,
        );

        self.runtime
            .execute_script("flux:invoke", invoke)
            .context("failed to invoke user handler")?;

        self.runtime
            .run_event_loop(Default::default())
            .await
            .context("failed while running JS event loop")?;

        let result_value = self
            .runtime
            .execute_script(
                "flux:result",
                "JSON.stringify({ result: globalThis.__flux_last_result ?? null, error: globalThis.__flux_last_error ?? null })",
            )
            .context("failed to read handler result")?;

        let raw: String = {
            let scope = &mut self.runtime.handle_scope();
            let local = deno_core::v8::Local::new(scope, result_value);
            deno_core::serde_v8::from_v8(scope, local)
                .context("failed to deserialize handler result")?
        };

        let envelope: serde_json::Value = serde_json::from_str(&raw)
            .context("handler result envelope is not valid JSON")?;

        if let Some(error) = envelope.get("error").and_then(|v| v.as_str()) {
            if !error.is_empty() {
                bail!(error.to_string());
            }
        }

        let checkpoints = {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let execution = state.borrow_mut::<RuntimeExecutionState>();
            std::mem::take(&mut execution.checkpoints)
        };

        Ok(JsExecutionOutput {
            output: envelope
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            checkpoints,
        })
    }
}

fn bootstrap_fetch_js() -> &'static str {
    r#"
globalThis.fetch = async function(url, init = {}) {
  const method = typeof init?.method === "string" ? init.method : "GET";
  const body = init?.body ?? null;
  const headers = init?.headers ?? null;
  const response = await Deno.core.ops.op_fetch(String(url), String(method), body, headers);

  return {
    status: response.status,
    ok: response.status >= 200 && response.status < 400,
    headers: response.headers ?? {},
    async json() {
      return response.body;
    },
    async text() {
      if (typeof response.body === "string") return response.body;
      return JSON.stringify(response.body ?? null);
    },
  };
};
"#
}

fn prepare_user_code(code: &str) -> String {
    let transformed = if code.contains("export default async function") {
        code.replacen(
            "export default async function",
            "globalThis.__flux_user_handler = async function",
            1,
        )
    } else if code.contains("export default function") {
        code.replacen(
            "export default function",
            "globalThis.__flux_user_handler = function",
            1,
        )
    } else if code.contains("export default") {
        code.replacen("export default", "globalThis.__flux_user_handler =", 1)
    } else {
        code.to_string()
    };

    format!(
        "{}\n\
         if (typeof globalThis.__flux_user_handler !== 'function') {{\n\
           throw new Error('entry module must export default function');\n\
         }}",
        transformed
    )
}
