use std::net::SocketAddr;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use axum::Router;
use axum::routing::get;
use base64::Engine as _;
use runtime::JsIsolate;
use runtime::artifact::build_artifact;
use runtime::deno_runtime::NetRequest;
use runtime::isolate_pool::{ExecutionContext, IsolatePool};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};

/// Decode a response body from the Flux runtime.
/// The runtime encodes binary/text bodies as `__FLUX_B64:<base64>` so they
/// survive JSON round-trips. This helper gives back the original UTF-8 string.
fn decode_body(raw: &str) -> String {
    if let Some(encoded) = raw.strip_prefix("__FLUX_B64:") {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap_or_default();
        String::from_utf8(bytes).unwrap_or_else(|_| raw.to_string())
    } else {
        raw.to_string()
    }
}

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
    assert!(
        pool.is_server_mode,
        "Deno.serve entry should initialize in server mode"
    );

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
    // The runtime encodes text response bodies as __FLUX_B64:<base64>.
    // Decode and assert the fields individually.
    let net_response = &result.body["net_response"];
    assert_eq!(net_response["status"], 202, "upstream status must be 202");
    assert_eq!(
        net_response["headers"],
        serde_json::json!([["content-type", "text/plain"]]),
        "content-type header must be forwarded"
    );
    let decoded = decode_body(net_response["body"].as_str().unwrap_or_default());
    assert_eq!(decoded, "buffered-response", "fetch response body must survive round-trip");
    assert_eq!(
        result.checkpoints.len(),
        1,
        "server-mode fetches should be recorded"
    );
    assert_eq!(
        result.logs.len(),
        1,
        "server-mode console logs should be preserved"
    );
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
    let raw_body = result.body["net_response"]["body"]
        .as_str()
        .context("server-mode response body should be a JSON string")?;
    let response_text = decode_body(raw_body);

    let payload: serde_json::Value =
        serde_json::from_str(&response_text).context("response should be valid JSON")?;
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
    let raw_body = result.body["net_response"]["body"]
        .as_str()
        .context("late-registration response body should be a JSON string")?;
    let response_text = decode_body(raw_body);
    let payload: serde_json::Value = serde_json::from_str(&response_text)
        .context("late-registration response should be valid JSON")?;
    assert_eq!(payload["ok"], true);
    assert_eq!(
        payload["error"],
        "Deno.serve may only be called during boot"
    );

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

    let mut isolate = JsIsolate::new_for_run(code).await.context("failed to create isolate")?;
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
    assert!(
        output.checkpoints.is_empty(),
        "pre-aborted fetch must not record a checkpoint"
    );

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

    let mut live_isolate = JsIsolate::new_for_run(code).await.context("failed to create live isolate")?;
    let live_output = live_isolate
        .execute(serde_json::json!({}), ExecutionContext::new("timer-live"))
        .await
        .context("live timer execution failed")?;

    assert_eq!(live_output.error, None);
    assert_eq!(live_output.output, serde_json::json!({ "ok": true }));
    assert_eq!(live_output.checkpoints.len(), 1);
    assert_eq!(live_output.checkpoints[0].boundary, "timer");
    assert_eq!(live_output.checkpoints[0].method, "delay");

    let mut replay_isolate =
        JsIsolate::new_for_run(code).await.context("failed to create replay isolate")?;
    let mut replay_context = ExecutionContext::new("timer-replay");
    replay_context.mode = runtime::deno_runtime::ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(
            serde_json::json!({}),
            replay_context,
            live_output.checkpoints.clone(),
        )
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

    let mut isolate = JsIsolate::new_for_run(code).await.context("failed to create isolate")?;
    let output = isolate
        .execute(
            serde_json::json!({}),
            ExecutionContext::new("wintertc-min-common"),
        )
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
    assert!(
        output.checkpoints.is_empty(),
        "wintertc globals should not create checkpoints"
    );
    assert_eq!(
        output.logs.len(),
        1,
        "reportError should emit exactly one console error"
    );
    assert!(output.logs[0].message.contains("wintertc report"));

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
// deno_crypto::op_crypto_verify_key triggers a non-unwinding panic (SIGABRT) on
// Apple Silicon macOS. This is a known interaction between deno_crypto-0.253.0 and
// the underlying OpenSSL/BoringSSL build on arm64-apple-darwin.
// CI runs on ubuntu-22.04 (amd64) where this test passes cleanly.
// Track: https://github.com/denoland/deno/issues (deno_crypto arm64 compat)
#[cfg_attr(target_os = "macos", ignore = "deno_crypto SIGABRT on Apple Silicon — passes in CI (Linux)")]
async fn crypto_subtle_import_key_and_verify_support_rs256_jwks() -> Result<()> {
    let _lock = polyfill_test_lock().lock().await;
    let code = format!(
                r#"
export default async function handler() {{
    const jwk = {jwk_json};
    const normalizedSignature = {signature:?}.replace(/-/g, "+").replace(/_/g, "/");
    const paddedSignature = normalizedSignature.padEnd(Math.ceil(normalizedSignature.length / 4) * 4, "=");
    const signatureBytes = Uint8Array.from(atob(paddedSignature), (char) => char.charCodeAt(0));
    const key = await crypto.subtle.importKey(
        "jwk",
        jwk,
        {{ name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }},
        false,
        ["verify"],
    );
    const verified = await crypto.subtle.verify(
        {{ name: "RSASSA-PKCS1-v1_5" }},
        key,
        signatureBytes,
        new TextEncoder().encode({message:?}),
    );

    return {{
        subtleType: typeof crypto.subtle,
        keyType: key.type,
        algorithm: key.algorithm.name,
        verified,
    }};
}}
"#,
                jwk_json = serde_json::json!({
                        "kty": "RSA",
                        "n": "sH2PCgllKet7j4tv8673SrBdTrwlKwmr419UrkJLbyFdg11LTDL0jdikNBEhu-1DSw2zxCXFvpXcjWnNpnhqI4EkO6o8YspnLpfLFwKudNeyIQo59jUFMU1naQCY5EEAdDdbcCEjxHhrLvcnekLYI4fhNQiIahYQyu0dc_Pmvkf69a6KJGJO5T__KcUbOiq__hsHlM4x9IYYxWpvLtNxWi1Fk6igFX5OEX4V5PaUmMH1z9RFTNqqVEpUr5S4_cotvTRuYyHYRsYZsRhJcJzLvthI74tVLPy7fNYdfzcnt_epJgtb14c8CMDTYK3N3Tz-UZy_tTVqYwSZQiAjSkAyzw",
                        "e": "AQAB",
                        "alg": "RS256",
                        "use": "sig",
                        "kid": "fixture-key",
                })
                .to_string(),
                signature = "ceny8CT3yclvlcBs0lY8gBRXmVg1Sjmcl0FrEjK2MXELbpXs4t9NpVwzu3FPFjfB_IrvU0_NDY4InHGjHchpbjn2RZIurlLEFJWk7Ms4K1-vLylqsOoKU1zIVKd5Dc1lMyseCzrvqKjF8Ffrzi8b6kjZhMdUuMIHk25pbCrEs33l3gT1Rts1Jq5--_VGJU1GJhWK19udxapEikfoG6vDq0PQq-DyylDX3tO_45o648QBGxm-ItBfJRwYcttsEP54bQ8FHkCHq0AFLi8VOUtWb5otA1qQuLWkw1B5hnzFtLvsiyQ8YaEQTYWmofGSMFEROHG8P7S1ShxcqOEvMxwkRw",
                message = "flux-rs256-fixture",
        );

    let mut isolate = JsIsolate::new_for_run(&code).await.context("failed to create isolate")?;
    let output = isolate
        .execute(
            serde_json::json!({}),
            ExecutionContext::new("crypto-subtle-rs256"),
        )
        .await
        .context("crypto.subtle rs256 execution failed")?;

    assert_eq!(output.error, None);
    assert_eq!(
        output.output,
        serde_json::json!({
                "subtleType": "object",
                "keyType": "public",
                "algorithm": "RSASSA-PKCS1-v1_5",
                "verified": true,
        })
    );
    assert!(
        output.checkpoints.is_empty(),
        "crypto.subtle verification should not create checkpoints"
    );

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

    let mut isolate = JsIsolate::new_for_run(code).await.context("failed to create isolate")?;
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

async fn spawn_test_server() -> Result<(
    String,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<()>>,
)> {
    let router = Router::new().route("/data", get(|| async { "buffered-response" }));

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
