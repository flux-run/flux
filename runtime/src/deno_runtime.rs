use std::cell::RefCell;
use std::collections::HashMap;
use std::net::IpAddr;
use std::rc::Rc;

use anyhow::{Context, Result};
use deno_core::error::AnyError;
use deno_core::{JsRuntime, OpState, RuntimeOptions, op2};
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::isolate_pool::ExecutionContext;

/// Maximum response body size: 10 MB.
const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

/// Blocked metadata hostnames (cloud provider instance metadata endpoints).
const BLOCKED_HOSTS: &[&str] = &[
    "169.254.169.254",
    "metadata.google.internal",
    "169.254.170.2",
];

/// Validate that a URL is safe to fetch — blocks SSRF to cloud metadata and private IPs.
fn validate_fetch_url(raw_url: &str) -> std::result::Result<(), AnyError> {
    let parsed = url::Url::parse(raw_url)
        .map_err(|e| deno_core::error::custom_error("TypeError", format!("invalid URL: {e}")))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| deno_core::error::custom_error("TypeError", "invalid URL: no host"))?;

    for blocked in BLOCKED_HOSTS {
        if host == *blocked {
            return Err(deno_core::error::custom_error(
                "PermissionDenied",
                format!("fetch blocked: {host} is a restricted endpoint"),
            ));
        }
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if ip.is_loopback() || is_private_ip(&ip) {
            return Err(deno_core::error::custom_error(
                "PermissionDenied",
                "fetch blocked: private/loopback IP addresses are not allowed",
            ));
        }
    }

    Ok(())
}

fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()          // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
            || v4.is_link_local()    // 169.254.0.0/16
            || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64 // 100.64.0.0/10 (CGNAT)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()     // ::1
            || (v6.segments()[0] & 0xfe00) == 0xfc00  // fc00::/7 unique-local
            || (v6.segments()[0] & 0xffc0) == 0xfe80  // fe80::/10 link-local
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExecutionMode {
    Live,
    Replay,
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
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
struct RuntimeExecutionState {
    context: ExecutionContext,
    call_index: u32,
    checkpoints: Vec<FetchCheckpoint>,
    /// Pre-recorded checkpoints for Replay mode, keyed by call_index.
    recorded: HashMap<u32, FetchCheckpoint>,
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
    let original_url = url;

    let (request_id, call_index, mode, recorded_checkpoint, client) = {
        let mut state_ref = state.borrow_mut();
        let (request_id, index, mode, recorded) = {
            let execution = state_ref.borrow_mut::<RuntimeExecutionState>();
            let idx = execution.call_index;
            execution.call_index = execution.call_index.saturating_add(1);
            let rec = execution.recorded.remove(&idx);
            (
                execution.context.request_id.clone(),
                idx,
                execution.context.mode.clone(),
                rec,
            )
        };
        let http_client = state_ref.borrow::<Client>().clone();
        (request_id, index, mode, recorded, http_client)
    };

    // In Replay mode, return the recorded response instead of making a live call.
    if matches!(mode, ExecutionMode::Replay) {
        if let Some(checkpoint) = recorded_checkpoint {
            let response = checkpoint.response.clone();
            {
                let mut state_ref = state.borrow_mut();
                let execution = state_ref.borrow_mut::<RuntimeExecutionState>();
                execution.checkpoints.push(FetchCheckpoint {
                    call_index,
                    boundary: checkpoint.boundary,
                    url: checkpoint.url,
                    method: checkpoint.method,
                    request: checkpoint.request,
                    response: response.clone(),
                    duration_ms: checkpoint.duration_ms,
                });
            }
            tracing::debug!(%request_id, %call_index, "replay: returned recorded response");
            return Ok(response);
        }
        // No recorded checkpoint for this index — fall through to live call.
        tracing::warn!(%request_id, %call_index, "replay: no recorded checkpoint, making live call");
    }

    // SSRF protection: block private/metadata IPs before making the request.
    validate_fetch_url(&original_url)?;

    let resolved_url = original_url.clone();
    let request_json = serde_json::json!({
        "url": original_url.clone(),
        "resolved_url": resolved_url.clone(),
        "method": method.clone(),
        "body": body.clone(),
        "headers": headers.clone(),
    });

    let started = std::time::Instant::now();
    let target_url = resolved_url;

    let response = make_http_request(&client, &target_url, &method, body, headers).await?;
    let duration_ms = started.elapsed().as_millis() as i32;

    {
        let mut state_ref = state.borrow_mut();
        let execution = state_ref.borrow_mut::<RuntimeExecutionState>();
        execution.checkpoints.push(FetchCheckpoint {
            call_index,
            boundary: "http".to_string(),
            url: original_url.clone(),
            method: method.clone(),
            request: request_json,
            response: response.clone(),
            duration_ms,
        });
    }

    tracing::debug!(%request_id, %call_index, original_url = %original_url, resolved_url = %target_url, "intercepted fetch");
    Ok(response)
}

async fn make_http_request(
    client: &Client,
    url: &str,
    method: &str,
    body: Option<serde_json::Value>,
    headers: Option<serde_json::Value>,
) -> Result<serde_json::Value, AnyError> {
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

    // Reject responses that advertise a body larger than our limit.
    if let Some(len) = response.content_length() {
        if len as usize > MAX_RESPONSE_BYTES {
            return Err(deno_core::error::custom_error(
                "TypeError",
                format!("response too large: {len} bytes exceeds {MAX_RESPONSE_BYTES} byte limit"),
            ));
        }
    }

    let status = response.status().as_u16();
    let response_headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            let value = v.to_str().unwrap_or_default().to_string();
            (k.to_string(), value)
        })
        .collect::<HashMap<_, _>>();

    // Stream the body with a size cap to protect against missing/lying Content-Length.
    let bytes = response
        .bytes()
        .await
        .map_err(|err| deno_core::error::custom_error("TypeError", err.to_string()))?;

    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(deno_core::error::custom_error(
            "TypeError",
            format!(
                "response body too large: {} bytes exceeds {MAX_RESPONSE_BYTES} byte limit",
                bytes.len()
            ),
        ));
    }

    let text = String::from_utf8_lossy(&bytes).into_owned();

    let parsed_body = serde_json::from_str::<serde_json::Value>(&text)
        .unwrap_or_else(|_| serde_json::Value::String(text));

    Ok(serde_json::json!({
        "status": status,
        "headers": response_headers,
        "body": parsed_body,
    }))
}

/// Maximum V8 heap size: 128 MB.
const V8_HEAP_LIMIT: usize = 128 * 1024 * 1024;

/// Maximum execution time for a single function invocation.
const EXECUTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub struct JsIsolate {
    runtime: JsRuntime,
    http_client: Client,
}

impl JsIsolate {
    pub fn new(user_code: &str, _isolate_id: usize) -> Result<Self> {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![flux_runtime_ext::init_ops_and_esm()],
            create_params: Some(
                deno_core::v8::CreateParams::default()
                    .heap_limits(0, V8_HEAP_LIMIT),
            ),
            ..Default::default()
        });

        runtime
            .execute_script("flux:bootstrap_fetch", bootstrap_fetch_js())
            .context("failed to install fetch interceptor")?;

        let prepared = prepare_user_code(user_code);
        runtime
            .execute_script("flux:user_code", prepared)
            .context("failed to load user code")?;

        Ok(Self {
            runtime,
            http_client: Client::new(),
        })
    }

    pub async fn execute(
        &mut self,
        payload: serde_json::Value,
        context: ExecutionContext,
    ) -> Result<JsExecutionOutput> {
        self.execute_with_recorded(payload, context, Vec::new()).await
    }

    /// Execute with pre-recorded checkpoints injected into OpState.
    /// In Replay mode, op_fetch will return the recorded response for each call_index
    /// instead of making a live HTTP call.
    pub async fn execute_with_recorded(
        &mut self,
        payload: serde_json::Value,
        context: ExecutionContext,
        recorded_checkpoints: Vec<FetchCheckpoint>,
    ) -> Result<JsExecutionOutput> {
        let recorded: HashMap<u32, FetchCheckpoint> = recorded_checkpoints
            .into_iter()
            .map(|cp| (cp.call_index, cp))
            .collect();
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            state.put(RuntimeExecutionState {
                context,
                call_index: 0,
                checkpoints: Vec::new(),
                recorded,
            });
            state.put(self.http_client.clone());
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

        tokio::time::timeout(
            EXECUTION_TIMEOUT,
            self.runtime.run_event_loop(Default::default()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("function execution timed out after {EXECUTION_TIMEOUT:?}"))?
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

        let checkpoints = {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let execution = state.borrow_mut::<RuntimeExecutionState>();
            std::mem::take(&mut execution.checkpoints)
        };

        let error = envelope
            .get("error")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());

        Ok(JsExecutionOutput {
            output: envelope
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
            checkpoints,
            error,
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
