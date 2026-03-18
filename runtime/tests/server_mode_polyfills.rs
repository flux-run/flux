use std::net::SocketAddr;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use axum::routing::get;
use axum::Router;
use runtime::artifact::build_artifact;
use runtime::deno_runtime::NetRequest;
use runtime::isolate_pool::{ExecutionContext, IsolatePool};
use runtime::JsIsolate;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_mode_captures_logs_and_fetch_checkpoints() -> Result<()> {
    let _lock = polyfill_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_FETCH", "1");
    let (base_url, shutdown_tx, server_task) = spawn_test_server().await?;

    let code = format!(
        r#"
Deno.serve(async function handler(_request) {{
  console.log("server hit");
  const response = await fetch({upstream:?});
  return new Response(await response.text(), {{
    status: 202,
    headers: {{ "content-type": "text/plain" }},
  }});
}});
"#,
        upstream = format!("{base_url}/data"),
    );

    let pool = IsolatePool::new(1, build_artifact("server.js", code))?;
    assert!(pool.is_server_mode, "Deno.serve entry should initialize in server mode");

    let result = pool
        .execute_net_request(
            ExecutionContext::new("server-mode-polyfill"),
            NetRequest {
                req_id: "req-1".to_string(),
                method: "GET".to_string(),
                url: "http://example.test/hello".to_string(),
                headers_json: "[]".to_string(),
                body: String::new(),
            },
        )
        .await;

    shutdown_tx.send(()).ok();
    server_task.await.context("test server task failed")??;

    assert_eq!(result.status, "ok");
    assert_eq!(
        result.body,
        serde_json::json!({
            "net_response": {
                "status": 202,
                "headers": [["content-type", "text/plain"]],
                "body": "buffered-response",
            }
        })
    );
    assert_eq!(result.checkpoints.len(), 1, "server-mode fetches should be recorded");
    assert_eq!(result.logs.len(), 1, "server-mode console logs should be preserved");
    assert!(result.logs[0].message.contains("server hit"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deno_serve_supports_options_overload_and_onlisten_callback() -> Result<()> {
    let _lock = polyfill_test_lock().lock().await;
    let code = r#"
let listenInfo = null;

Deno.serve(
  {
    hostname: "127.0.0.1",
    port: 4321,
    onListen(info) {
      listenInfo = info;
    },
  },
  () => Response.json({ listenInfo }),
);
"#;

    let pool = IsolatePool::new(1, build_artifact("server.js", code))?;
    let result = pool
        .execute_net_request(
            ExecutionContext::new("deno-serve-onlisten"),
            NetRequest {
                req_id: "req-onlisten".to_string(),
                method: "GET".to_string(),
                url: "http://example.test/hello".to_string(),
                headers_json: "[]".to_string(),
                body: String::new(),
            },
        )
        .await;

    assert_eq!(result.status, "ok");
    let response_text = result.body["net_response"]["body"]
        .as_str()
        .context("server-mode response body should be a JSON string")?;

    let payload: serde_json::Value = serde_json::from_str(&response_text)
        .context("response should be valid JSON")?;
    assert_eq!(
        payload,
        serde_json::json!({
            "listenInfo": {
                "hostname": "127.0.0.1",
                "port": 4321,
            }
        })
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deno_serve_honors_preaborted_signal() -> Result<()> {
    let _lock = polyfill_test_lock().lock().await;
    let code = r#"
const controller = new AbortController();
controller.abort("shutdown requested");

Deno.serve({ signal: controller.signal }, () => new Response("should not run"));
"#;

    let pool = IsolatePool::new(1, build_artifact("server.js", code))?;
    let result = pool
        .execute_net_request(
            ExecutionContext::new("deno-serve-abort"),
            NetRequest {
                req_id: "req-2".to_string(),
                method: "GET".to_string(),
                url: "http://example.test/closed".to_string(),
                headers_json: "[]".to_string(),
                body: String::new(),
            },
        )
        .await;

    assert_eq!(result.status, "ok");
    assert_eq!(
        result.body,
        serde_json::json!({
            "net_response": {
                "status": 503,
                "headers": [],
                "body": "shutdown requested",
            }
        })
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deno_serve_rejects_duplicate_listener_registration_during_boot() -> Result<()> {
    let _lock = polyfill_test_lock().lock().await;
    let artifact = build_artifact(
        "server.js",
        r#"
Deno.serve(() => new Response("first"));
Deno.serve(() => new Response("second"));
"#,
    );

    let boot = runtime::boot_runtime_artifact(
        &artifact,
        ExecutionContext::new(artifact.code_version().to_string()),
    )
    .await?;

    assert_eq!(boot.result.status, "error");
    assert!(
        boot.result
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("Deno.serve may only register one listener during boot")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deno_serve_rejects_listener_registration_after_boot() -> Result<()> {
    let _lock = polyfill_test_lock().lock().await;
    let code = r#"
Deno.serve((_req) => {
  try {
    Deno.serve(() => new Response("late"));
    return Response.json({ ok: false });
  } catch (err) {
    return Response.json({
      ok: true,
      error: String(err && err.message ? err.message : err),
    });
  }
});
"#;

    let pool = IsolatePool::new(1, build_artifact("server.js", code))?;
    let result = pool
        .execute_net_request(
            ExecutionContext::new("deno-serve-late-register"),
            NetRequest {
                req_id: "req-late-register".to_string(),
                method: "GET".to_string(),
                url: "http://example.test/hello".to_string(),
                headers_json: "[]".to_string(),
                body: String::new(),
            },
        )
        .await;

    assert_eq!(result.status, "ok");
    let response_text = result.body["net_response"]["body"]
        .as_str()
        .context("late-registration response body should be a JSON string")?;
    let payload: serde_json::Value = serde_json::from_str(response_text)
        .context("late-registration response should be valid JSON")?;
    assert_eq!(payload["ok"], true);
    assert_eq!(payload["error"], "Deno.serve may only be called during boot");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_rejects_preaborted_signal_without_network_call() -> Result<()> {
    let _lock = polyfill_test_lock().lock().await;
    let code = r#"
export default async function handler() {
  const controller = new AbortController();
  controller.abort();

  try {
    await fetch("https://example.com", { signal: controller.signal });
    return { ok: true };
  } catch (err) {
    return {
      ok: false,
      aborted: controller.signal.aborted,
      name: err?.name ?? null,
      message: err?.message ?? null,
      string: String(err),
    };
  }
}
"#;

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create isolate")?;
    let output = isolate
        .execute(serde_json::json!({}), ExecutionContext::new("fetch-abort"))
        .await
        .context("fetch abort execution failed")?;

    assert_eq!(output.error, None);
    assert_eq!(
        output.output,
        serde_json::json!({
            "ok": false,
            "aborted": true,
            "name": "AbortError",
            "message": "This operation was aborted",
            "string": "AbortError: This operation was aborted",
        })
    );
    assert!(output.checkpoints.is_empty(), "pre-aborted fetch must not record a checkpoint");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn timer_boundaries_are_recorded_and_replayed() -> Result<()> {
    let _lock = polyfill_test_lock().lock().await;
    let code = r#"
export default async function handler() {
  await new Promise((resolve) => setTimeout(resolve, 10));
  return { ok: true };
}
"#;

    let mut live_isolate = JsIsolate::new_for_run(code).context("failed to create live isolate")?;
    let live_output = live_isolate
        .execute(serde_json::json!({}), ExecutionContext::new("timer-live"))
        .await
        .context("live timer execution failed")?;

    assert_eq!(live_output.error, None);
    assert_eq!(live_output.output, serde_json::json!({ "ok": true }));
    assert_eq!(live_output.checkpoints.len(), 1);
    assert_eq!(live_output.checkpoints[0].boundary, "timer");
    assert_eq!(live_output.checkpoints[0].method, "delay");

    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create replay isolate")?;
    let mut replay_context = ExecutionContext::new("timer-replay");
    replay_context.mode = runtime::deno_runtime::ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(serde_json::json!({}), replay_context, live_output.checkpoints.clone())
        .await
        .context("replay timer execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(replay_output.output, serde_json::json!({ "ok": true }));
    assert_eq!(replay_output.checkpoints.len(), 1);
    assert_eq!(replay_output.checkpoints[0].boundary, "timer");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wintertc_minimum_common_globals_are_available() -> Result<()> {
    let _lock = polyfill_test_lock().lock().await;
    let code = r#"
export default async function handler() {
  reportError(new Error("wintertc report"));

  return {
    selfIsGlobal: self === globalThis,
    navigatorType: typeof navigator,
    userAgent: navigator.userAgent,
    reportErrorType: typeof reportError,
    encoded: btoa("Flux"),
    decoded: atob("Rmx1eA=="),
  };
}
"#;

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create isolate")?;
    let output = isolate
        .execute(serde_json::json!({}), ExecutionContext::new("wintertc-min-common"))
        .await
        .context("wintertc globals execution failed")?;

    assert_eq!(output.error, None);
    assert_eq!(
        output.output,
        serde_json::json!({
            "selfIsGlobal": true,
            "navigatorType": "object",
            "userAgent": "Flux Runtime",
            "reportErrorType": "function",
            "encoded": "Rmx1eA==",
            "decoded": "Flux",
        })
    );
    assert!(output.checkpoints.is_empty(), "wintertc globals should not create checkpoints");
    assert_eq!(output.logs.len(), 1, "reportError should emit exactly one console error");
    assert!(output.logs[0].message.contains("wintertc report"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn deno_env_get_reads_process_environment() -> Result<()> {
    let _lock = polyfill_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLUX_TEST_DATABASE_URL", "postgres://flux:test@db/flux");
    let code = r#"
export default async function handler() {
  return {
    value: Deno.env.get("FLUX_TEST_DATABASE_URL") ?? null,
    missing: Deno.env.get("FLUX_TEST_MISSING") ?? null,
  };
}
"#;

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create isolate")?;
    let output = isolate
        .execute(serde_json::json!({}), ExecutionContext::new("deno-env-get"))
        .await
        .context("deno env get execution failed")?;

    assert_eq!(output.error, None);
    assert_eq!(
        output.output,
        serde_json::json!({
            "value": "postgres://flux:test@db/flux",
            "missing": null,
        })
    );

    Ok(())
}

fn polyfill_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn spawn_test_server() -> Result<(String, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let router = Router::new()
        .route("/data", get(|| async { "buffered-response" }));

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind loopback test server")?;
    let addr: SocketAddr = listener
        .local_addr()
        .context("failed to read test server address")?;
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let handle = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .context("axum test server failed")?;
        Ok(())
    });

    Ok((format!("http://{}", addr), shutdown_tx, handle))
}

struct EnvVarGuard {
    key: &'static str,
    original: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let original = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.original {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}