use std::cell::RefCell;
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

#[derive(Debug, Clone)]
struct RuntimeExecutionState {
    context: ExecutionContext,
    call_index: u32,
}

deno_core::extension!(flux_runtime_ext, ops = [op_fetch]);

#[op2(async)]
#[serde]
async fn op_fetch(
    state: Rc<RefCell<OpState>>,
    #[string] url: String,
    #[string] method: String,
    #[serde] body: Option<serde_json::Value>,
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
            let client = Client::new();
            let method = method.parse::<reqwest::Method>()
                .map_err(|err| deno_core::error::custom_error("TypeError", err.to_string()))?;

            let mut request = client.request(method, &url);
            if let Some(body) = body {
                request = request.json(&body);
            }

            let response = request.send().await.map_err(|err| {
                deno_core::error::custom_error("TypeError", format!("fetch failed: {err}"))
            })?;

            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .map(|(k, v)| {
                    let value = v.to_str().unwrap_or_default().to_string();
                    (k.to_string(), value)
                })
                .collect::<std::collections::HashMap<_, _>>();

            let body_json = response
                .json::<serde_json::Value>()
                .await
                .unwrap_or(serde_json::Value::Null);

            tracing::debug!(%request_id, %call_index, %url, status, "intercepted fetch");

            Ok(serde_json::json!({
                "status": status,
                "headers": headers,
                "body": body_json,
            }))
        }
    }
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
    ) -> Result<serde_json::Value> {
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            state.put(RuntimeExecutionState {
                context,
                call_index: 0,
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

        let scope = &mut self.runtime.handle_scope();
        let local = deno_core::v8::Local::new(scope, result_value);
        let raw: String = deno_core::serde_v8::from_v8(scope, local)
            .context("failed to deserialize handler result")?;

        let envelope: serde_json::Value = serde_json::from_str(&raw)
            .context("handler result envelope is not valid JSON")?;

        if let Some(error) = envelope.get("error").and_then(|v| v.as_str()) {
            if !error.is_empty() {
                bail!(error.to_string());
            }
        }

        Ok(envelope.get("result").cloned().unwrap_or(serde_json::Value::Null))
    }
}

fn bootstrap_fetch_js() -> &'static str {
    r#"
globalThis.fetch = async function(url, init = {}) {
  const method = typeof init?.method === "string" ? init.method : "GET";
  const body = init?.body ?? null;
    const response = await Deno.core.ops.op_fetch(String(url), String(method), body);

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
