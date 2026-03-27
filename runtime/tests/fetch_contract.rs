use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use axum::extract::Query;
use axum::routing::get;
use axum::Router;
use runtime::deno_runtime::{reset_http_response_cache_for_tests, ExecutionMode, FetchCheckpoint};
use runtime::isolate_pool::ExecutionContext;
use runtime::JsIsolate;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, Mutex};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_replay_returns_buffered_body_via_reader() -> Result<()> {
    let _lock = fetch_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLUXBASE_ALLOW_LOOPBACK_FETCH", "1");

    let (base_url, shutdown_tx, server_task) = spawn_test_server().await?;

    let code = r#"
export default async function handler({ input }) {
    try {
    const response = await fetch(input.url);
        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        const chunks = [];

        while (true) {
            const { value, done } = await reader.read();
            if (done) break;
            chunks.push(decoder.decode(value, { stream: true }));
        }

        chunks.push(decoder.decode());

        return {
            ok: true,
            status: response.status,
            body: chunks.join(""),
            chunkCount: chunks.filter((chunk) => chunk.length > 0).length,
        };
    } catch (err) {
        return {
            ok: false,
            name: err?.name ?? null,
            message: err?.message ?? null,
            string: String(err),
            stack: err?.stack ?? null,
        };
    }
}
"#;

    let payload = serde_json::json!({
        "url": format!("{base_url}/data"),
    });

    let mut isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create live isolate")?;
    let live_context = ExecutionContext::new("fetch-contract-live");
    let live_output = isolate
        .execute(payload.clone(), live_context)
        .await
        .context("live execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("test server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "ok": true,
            "status": 200,
            "body": "buffered-response",
            "chunkCount": 1,
        })
    );

    let recorded = live_output.checkpoints.clone();
    assert_eq!(
        recorded.len(),
        1,
        "expected exactly one recorded fetch checkpoint"
    );
    assert_eq!(
        recorded[0].response.get("status"),
        Some(&serde_json::json!(200))
    );
    assert_eq!(
        recorded[0].response.get("body"),
        Some(&serde_json::json!("buffered-response"))
    );

    let mut replay_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create replay isolate")?;
    let mut replay_context = ExecutionContext::new("fetch-contract-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("replay execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(replay_output.output, live_output.output);
    assert_eq!(replay_output.checkpoints.len(), 1);
    assert_eq!(
        replay_output.checkpoints[0].response.get("status"),
        Some(&serde_json::json!(200))
    );
    assert_eq!(
        replay_output.checkpoints[0].response.get("body"),
        Some(&serde_json::json!("buffered-response"))
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_blocks_ssrf_targets_and_never_replays_them() -> Result<()> {
    let _lock = fetch_test_lock().lock().await;
    let code = r#"
export default async function handler({ input }) {
    try {
        const response = await fetch(input.url);
        return {
            ok: true,
            status: response.status,
            body: await response.text(),
        };
    } catch (err) {
        return {
            ok: false,
            name: err?.name ?? null,
            message: err?.message ?? null,
            string: String(err),
        };
    }
}
"#;

    let payload = serde_json::json!({
        "url": "http://169.254.169.254/latest/meta-data",
    });

    let mut live_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create live isolate")?;
    let live_output = live_isolate
        .execute(payload.clone(), ExecutionContext::new("fetch-ssrf-live"))
        .await
        .context("live SSRF execution failed")?;

    assert_ssrf_result(&live_output.output, "169.254.169.254");
    assert_eq!(live_output.error, None);
    assert!(
        live_output.checkpoints.is_empty(),
        "blocked fetches must not be recorded"
    );

    let fake_recording = vec![FetchCheckpoint {
        call_index: 0,
        boundary: "http".to_string(),
        url: "http://169.254.169.254/latest/meta-data".to_string(),
        method: "GET".to_string(),
        request: serde_json::json!({
            "url": "http://169.254.169.254/latest/meta-data",
            "method": "GET",
            "headers": {},
            "body": null,
        }),
        response: serde_json::json!({
            "status": 200,
            "headers": {"content-type": "text/plain"},
            "body": "should-never-replay",
        }),
        duration_ms: 0,
    }];

    let mut replay_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create replay isolate")?;
    let mut replay_context = ExecutionContext::new("fetch-ssrf-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, fake_recording)
        .await
        .context("replay SSRF execution failed")?;

    assert_ssrf_result(&replay_output.output, "169.254.169.254");
    assert_eq!(replay_output.error, None);
    assert!(
        replay_output.checkpoints.is_empty(),
        "blocked replay fetches must not be recorded"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_blocks_loopback_by_default_but_allows_it_in_test_mode() -> Result<()> {
    let _lock = fetch_test_lock().lock().await;
    let (base_url, shutdown_tx, server_task) = spawn_test_server().await?;
    let localhost_url = base_url.replace("127.0.0.1", "localhost");
    let code = r#"
export default async function handler({ input }) {
    try {
        const response = await fetch(input.url);
        return {
            ok: true,
            status: response.status,
            body: await response.text(),
        };
    } catch (err) {
        return {
            ok: false,
            name: err?.name ?? null,
            message: err?.message ?? null,
            string: String(err),
        };
    }
}
"#;

    let blocked_payload = serde_json::json!({ "url": format!("{localhost_url}/data") });
    let mut blocked_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create blocked isolate")?;
    let blocked_output = blocked_isolate
        .execute(
            blocked_payload.clone(),
            ExecutionContext::new("fetch-loopback-blocked"),
        )
        .await
        .context("blocked loopback execution failed")?;

    assert_eq!(blocked_output.error, None);
    let blocked_result = &blocked_output.output;
    assert_eq!(blocked_result.get("ok"), Some(&serde_json::json!(false)));
    let blocked_message = blocked_result
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let blocked_string = blocked_result
        .get("string")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    assert!(blocked_message.contains("fetch blocked") || blocked_string.contains("fetch blocked"));
    assert!(
        blocked_message.contains("private/loopback") || blocked_string.contains("private/loopback")
    );
    assert!(
        blocked_output.checkpoints.is_empty(),
        "blocked loopback fetch must not be recorded"
    );

    let _guard = EnvVarGuard::set("FLUXBASE_ALLOW_LOOPBACK_FETCH", "1");
    let mut allowed_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create allowed isolate")?;
    let allowed_output = allowed_isolate
        .execute(
            blocked_payload,
            ExecutionContext::new("fetch-loopback-allowed"),
        )
        .await
        .context("allowed loopback execution failed")?;

    assert_eq!(allowed_output.error, None);
    assert_eq!(
        allowed_output.output,
        serde_json::json!({
            "ok": true,
            "status": 200,
            "body": "buffered-response",
        })
    );
    assert_eq!(
        allowed_output.checkpoints.len(),
        1,
        "allowed loopback fetch should be recorded"
    );

    shutdown_tx.send(()).ok();
    server_task.await.context("test server task failed")??;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_redirects_are_revalidated_before_following() -> Result<()> {
    let _lock = fetch_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLUXBASE_ALLOW_LOOPBACK_FETCH", "1");
    let (base_url, shutdown_tx, server_task) = spawn_test_server().await?;
    let code = r#"
export default async function handler({ input }) {
    try {
        const response = await fetch(input.url);
        return {
            ok: true,
            body: await response.text(),
        };
    } catch (err) {
        return {
            ok: false,
            name: err?.name ?? null,
            message: err?.message ?? null,
            string: String(err),
        };
    }
}
"#;

    let payload = serde_json::json!({
        "url": format!("{base_url}/redirect-metadata"),
    });

    let mut isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create redirect isolate")?;
    let output = isolate
        .execute(payload, ExecutionContext::new("fetch-redirect-ssrf"))
        .await
        .context("redirect SSRF execution failed")?;

    assert_eq!(output.error, None);
    assert_ssrf_result(&output.output, "169.254.169.254");
    assert!(
        output.checkpoints.is_empty(),
        "redirected SSRF fetch must not be recorded"
    );

    shutdown_tx.send(()).ok();
    server_task.await.context("test server task failed")??;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_reuses_standard_cache_control_across_live_executions() -> Result<()> {
    let _lock = fetch_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLUXBASE_ALLOW_LOOPBACK_FETCH", "1");
    let _cache = CacheResetGuard::new();
    let (base_url, shutdown_tx, server_task, hit_count) = spawn_cache_test_server().await?;

    let code = r#"
export default async function handler({ input }) {
    const response = await fetch(input.url);
    return {
        status: response.status,
        body: await response.json(),
        cacheControl: response.headers.get("cache-control"),
    };
}
"#;

    let payload = serde_json::json!({
        "url": format!("{base_url}/cache"),
    });

    let mut first_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create first cache isolate")?;
    let first_output = first_isolate
        .execute(
            payload.clone(),
            ExecutionContext::new("fetch-http-cache-live-1"),
        )
        .await
        .context("first cached fetch execution failed")?;

    assert_eq!(first_output.error, None);
    assert_eq!(
        first_output.output,
        serde_json::json!({
            "status": 200,
            "body": { "requestCount": 1, "value": "cached-response" },
            "cacheControl": "public, max-age=600",
        })
    );
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);

    shutdown_tx.send(()).ok();
    server_task
        .await
        .context("cache test server task failed")??;

    let mut second_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create second cache isolate")?;
    let second_output = second_isolate
        .execute(payload, ExecutionContext::new("fetch-http-cache-live-2"))
        .await
        .context("second cached fetch execution failed")?;

    assert_eq!(second_output.error, None);
    assert_eq!(second_output.output, first_output.output);
    assert_eq!(second_output.checkpoints.len(), 1);
    assert_cache_hit_metadata(&second_output.checkpoints[0].response);
    assert_eq!(
        hit_count.load(Ordering::SeqCst),
        1,
        "second execution should reuse the in-memory cache"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_request_no_cache_bypasses_shared_memory_cache() -> Result<()> {
    let _lock = fetch_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLUXBASE_ALLOW_LOOPBACK_FETCH", "1");
    let _cache = CacheResetGuard::new();
    let (base_url, shutdown_tx, server_task, hit_count) = spawn_cache_test_server().await?;

    let prime_code = r#"
export default async function handler({ input }) {
    const response = await fetch(input.url);
    return {
        status: response.status,
        body: await response.json(),
    };
}
"#;

    let bypass_code = r#"
export default async function handler({ input }) {
    try {
        const response = await fetch(input.url, {
            headers: {
                "cache-control": "no-cache",
            },
        });
        return {
            ok: true,
            status: response.status,
            body: await response.text(),
        };
    } catch (err) {
        return {
            ok: false,
            message: err?.message ?? null,
            string: String(err),
        };
    }
}
"#;

    let payload = serde_json::json!({
        "url": format!("{base_url}/cache"),
    });

    let mut prime_isolate = JsIsolate::new_for_run(prime_code)
        .await
        .context("failed to create cache prime isolate")?;
    let prime_output = prime_isolate
        .execute(
            payload.clone(),
            ExecutionContext::new("fetch-http-cache-prime"),
        )
        .await
        .context("cache prime execution failed")?;

    assert_eq!(prime_output.error, None);
    assert_eq!(
        prime_output.output.get("status"),
        Some(&serde_json::json!(200))
    );
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);

    shutdown_tx.send(()).ok();
    server_task
        .await
        .context("cache bypass test server task failed")??;

    let mut bypass_isolate = JsIsolate::new_for_run(bypass_code)
        .await
        .context("failed to create cache bypass isolate")?;
    let bypass_output = bypass_isolate
        .execute(payload, ExecutionContext::new("fetch-http-cache-bypass"))
        .await
        .context("cache bypass execution failed")?;

    assert_eq!(bypass_output.error, None);
    assert_eq!(
        bypass_output.output.get("ok"),
        Some(&serde_json::json!(false))
    );
    let message = bypass_output
        .output
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let string = bypass_output
        .output
        .get("string")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    assert!(message.contains("fetch failed") || string.contains("fetch failed"));
    assert_eq!(
        hit_count.load(Ordering::SeqCst),
        1,
        "bypass request should not reuse or refresh the in-memory cache after shutdown"
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_cache_evicts_least_recently_used_entry_when_entry_limit_is_reached() -> Result<()> {
    let _lock = fetch_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLUXBASE_ALLOW_LOOPBACK_FETCH", "1");
    let _max_entries = EnvVarGuard::set("FLUXBASE_HTTP_CACHE_MAX_ENTRIES", "1");
    let _cache = CacheResetGuard::new();
    let (base_url, shutdown_tx, server_task, hit_count) = spawn_cache_test_server().await?;

    let code = r#"
export default async function handler({ input }) {
    try {
        const response = await fetch(input.url);
        return {
            ok: true,
            status: response.status,
            body: await response.json(),
        };
    } catch (err) {
        return {
            ok: false,
            message: err?.message ?? null,
            string: String(err),
        };
    }
}
"#;

    let first_payload = serde_json::json!({
        "url": format!("{base_url}/cache?slot=first"),
    });
    let second_payload = serde_json::json!({
        "url": format!("{base_url}/cache?slot=second"),
    });

    let mut first_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create first LRU isolate")?;
    let first_output = first_isolate
        .execute(
            first_payload.clone(),
            ExecutionContext::new("fetch-http-cache-lru-first"),
        )
        .await
        .context("first LRU execution failed")?;
    assert_eq!(first_output.error, None);
    assert_eq!(
        first_output.output.get("ok"),
        Some(&serde_json::json!(true))
    );

    let mut second_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create second LRU isolate")?;
    let second_output = second_isolate
        .execute(
            second_payload.clone(),
            ExecutionContext::new("fetch-http-cache-lru-second"),
        )
        .await
        .context("second LRU execution failed")?;
    assert_eq!(second_output.error, None);
    assert_eq!(
        second_output.output.get("ok"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(hit_count.load(Ordering::SeqCst), 2);

    shutdown_tx.send(()).ok();
    server_task
        .await
        .context("LRU cache test server task failed")??;

    let mut cached_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create retained-cache isolate")?;
    let cached_output = cached_isolate
        .execute(
            second_payload,
            ExecutionContext::new("fetch-http-cache-lru-retained"),
        )
        .await
        .context("retained cache execution failed")?;
    assert_eq!(cached_output.error, None);
    assert_eq!(
        cached_output.output.get("ok"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(cached_output.checkpoints.len(), 1);
    assert_cache_hit_metadata(&cached_output.checkpoints[0].response);

    let mut evicted_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create evicted-cache isolate")?;
    let evicted_output = evicted_isolate
        .execute(
            first_payload,
            ExecutionContext::new("fetch-http-cache-lru-evicted"),
        )
        .await
        .context("evicted cache execution failed")?;
    assert_eq!(evicted_output.error, None);
    assert_eq!(
        evicted_output.output.get("ok"),
        Some(&serde_json::json!(false))
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_cache_skips_entries_that_exceed_memory_budget() -> Result<()> {
    let _lock = fetch_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLUXBASE_ALLOW_LOOPBACK_FETCH", "1");
    let _max_bytes = EnvVarGuard::set("FLUXBASE_HTTP_CACHE_MAX_BYTES", "128");
    let _cache = CacheResetGuard::new();
    let (base_url, shutdown_tx, server_task, hit_count) = spawn_cache_test_server().await?;

    let code = r#"
export default async function handler({ input }) {
    try {
        const response = await fetch(input.url);
        return {
            ok: true,
            status: response.status,
            body: await response.json(),
        };
    } catch (err) {
        return {
            ok: false,
            message: err?.message ?? null,
            string: String(err),
        };
    }
}
"#;

    let payload = serde_json::json!({
        "url": format!("{base_url}/cache?size=large"),
    });

    let mut prime_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create large-response cache isolate")?;
    let prime_output = prime_isolate
        .execute(
            payload.clone(),
            ExecutionContext::new("fetch-http-cache-large-prime"),
        )
        .await
        .context("large-response cache prime failed")?;
    assert_eq!(prime_output.error, None);
    assert_eq!(
        prime_output.output.get("ok"),
        Some(&serde_json::json!(true))
    );
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);

    shutdown_tx.send(()).ok();
    server_task
        .await
        .context("large-response cache test server task failed")??;

    let mut second_isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create large-response replay isolate")?;
    let second_output = second_isolate
        .execute(
            payload,
            ExecutionContext::new("fetch-http-cache-large-after-shutdown"),
        )
        .await
        .context("large-response cache follow-up failed")?;

    assert_eq!(second_output.error, None);
    assert_eq!(
        second_output.output.get("ok"),
        Some(&serde_json::json!(false))
    );
    assert_eq!(hit_count.load(Ordering::SeqCst), 1);

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_body_consumption_and_clone_follow_web_semantics() -> Result<()> {
    let _lock = fetch_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLUXBASE_ALLOW_LOOPBACK_FETCH", "1");
    let (base_url, shutdown_tx, server_task) = spawn_test_server().await?;
    let code = r#"
export default async function handler({ input }) {
  const response = await fetch(input.url);
  const clone = response.clone();
  const parsed = await response.json();

  let textAfterJsonError = null;
  try {
    await response.text();
  } catch (err) {
    textAfterJsonError = String(err);
  }

  let cloneAfterConsumptionError = null;
  try {
    response.clone();
  } catch (err) {
    cloneAfterConsumptionError = String(err);
  }

  const cloneText = await clone.text();

  return {
    parsed,
    bodyUsed: response.bodyUsed,
    textAfterJsonError,
    cloneAfterConsumptionError,
    cloneText,
  };
}
"#;

    let payload = serde_json::json!({
        "url": format!("{base_url}/json"),
    });

    let mut isolate = JsIsolate::new_for_run(code)
        .await
        .context("failed to create body contract isolate")?;
    let output = isolate
        .execute(payload, ExecutionContext::new("fetch-body-contract"))
        .await
        .context("body contract execution failed")?;

    assert_eq!(output.error, None);
    assert_eq!(
        output.output,
        serde_json::json!({
            "parsed": {"ok": true, "message": "buffered-json"},
            "bodyUsed": true,
            "textAfterJsonError": "TypeError: Body already consumed",
            "cloneAfterConsumptionError": "TypeError: Body already consumed",
            "cloneText": "{\"ok\":true,\"message\":\"buffered-json\"}",
        })
    );
    assert_eq!(
        output.checkpoints.len(),
        1,
        "body contract fetch should be recorded"
    );

    shutdown_tx.send(()).ok();
    server_task.await.context("test server task failed")??;

    Ok(())
}

fn assert_ssrf_result(result: &serde_json::Value, needle: &str) {
    assert_eq!(result.get("ok"), Some(&serde_json::json!(false)));
    let message = result
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let string = result
        .get("string")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    assert!(
        message.contains("fetch blocked") || string.contains("fetch blocked"),
        "expected SSRF block result, got: {result}"
    );
    assert!(
        message.contains(needle) || string.contains(needle),
        "expected blocked target in result, got: {result}"
    );
}

fn assert_cache_hit_metadata(response: &serde_json::Value) {
    let cache = response
        .get("cache")
        .unwrap_or_else(|| panic!("expected cache metadata in response: {response}"));

    assert_eq!(cache.get("hit"), Some(&serde_json::json!(true)));
    assert_eq!(cache.get("source"), Some(&serde_json::json!("memory")));
    assert!(
        cache
            .get("age_ms")
            .and_then(|value| value.as_u64())
            .is_some(),
        "expected numeric age_ms in cache metadata: {cache}"
    );
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
            Some(value) => unsafe {
                std::env::set_var(self.key, value);
            },
            None => unsafe {
                std::env::remove_var(self.key);
            },
        }
    }
}

struct CacheResetGuard;

impl CacheResetGuard {
    fn new() -> Self {
        reset_http_response_cache_for_tests();
        Self
    }
}

impl Drop for CacheResetGuard {
    fn drop(&mut self) {
        reset_http_response_cache_for_tests();
    }
}

fn fetch_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn spawn_test_server() -> Result<(
    String,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<()>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind test server")?;
    let addr: SocketAddr = listener
        .local_addr()
        .context("failed to read test server addr")?;

    let app = Router::new()
        .route(
            "/data",
            get(|| async { ([("content-length", "17")], "buffered-response") }),
        )
        .route(
            "/json",
            get(|| async {
                (
                    [
                        ("content-type", "application/json"),
                        ("content-length", "37"),
                    ],
                    r#"{"ok":true,"message":"buffered-json"}"#,
                )
            }),
        )
        .route(
            "/redirect-metadata",
            get(|| async {
                (
                    axum::http::StatusCode::FOUND,
                    [("location", "http://169.254.169.254/latest/meta-data")],
                    "redirecting",
                )
            }),
        );

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .context("test server exited with error")?;
        Ok(())
    });

    Ok((format!("http://{addr}"), shutdown_tx, task))
}

async fn spawn_cache_test_server() -> Result<(
    String,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<()>>,
    Arc<AtomicUsize>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind cache test server")?;
    let addr: SocketAddr = listener
        .local_addr()
        .context("failed to read cache test server addr")?;
    let hit_count = Arc::new(AtomicUsize::new(0));
    let route_hits = Arc::clone(&hit_count);

    let app = Router::new().route(
        "/cache",
        get(move |Query(query): Query<HashMap<String, String>>| {
            let route_hits = Arc::clone(&route_hits);
            async move {
                let request_count = route_hits.fetch_add(1, Ordering::SeqCst) + 1;
                let value = if query.get("size").map(String::as_str) == Some("large") {
                    "x".repeat(512)
                } else {
                    "cached-response".to_string()
                };
                (
                    [
                        ("content-type", "application/json"),
                        ("cache-control", "public, max-age=600"),
                    ],
                    serde_json::json!({
                        "requestCount": request_count,
                        "value": value,
                    })
                    .to_string(),
                )
            }
        }),
    );

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await
            .context("cache test server exited with error")?;
        Ok(())
    });

    Ok((format!("http://{addr}"), shutdown_tx, task, hit_count))
}
