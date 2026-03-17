use std::net::SocketAddr;

use anyhow::{Context, Result};
use axum::Router;
use axum::routing::get;
use runtime::JsIsolate;
use runtime::deno_runtime::ExecutionMode;
use runtime::isolate_pool::ExecutionContext;
use tokio::net::TcpListener;
use tokio::sync::oneshot;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fetch_replay_returns_buffered_body_via_reader() -> Result<()> {
    unsafe {
        std::env::set_var("FLOWBASE_ALLOW_LOOPBACK_FETCH", "1");
    }

    let (base_url, shutdown_tx, server_task) = spawn_test_server().await?;

        let code = r#"
export default async function handler({ input }) {
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
    status: response.status,
    body: chunks.join(""),
    chunkCount: chunks.filter((chunk) => chunk.length > 0).length,
  };
}
"#;

    let payload = serde_json::json!({
        "url": format!("{base_url}/data"),
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create live isolate")?;
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
            "status": 200,
            "body": "buffered-response",
            "chunkCount": 1,
        })
    );

    let recorded = live_output.checkpoints.clone();
    assert_eq!(recorded.len(), 1, "expected exactly one recorded fetch checkpoint");
    assert_eq!(recorded[0].response.get("status"), Some(&serde_json::json!(200)));
    assert_eq!(
        recorded[0].response.get("body"),
        Some(&serde_json::json!("buffered-response"))
    );

    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create replay isolate")?;
    let mut replay_context = ExecutionContext::new("fetch-contract-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("replay execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(replay_output.output, live_output.output);
    assert_eq!(replay_output.checkpoints.len(), 1);
    assert_eq!(replay_output.checkpoints[0].response.get("status"), Some(&serde_json::json!(200)));
    assert_eq!(
        replay_output.checkpoints[0].response.get("body"),
        Some(&serde_json::json!("buffered-response"))
    );

    unsafe {
        std::env::remove_var("FLOWBASE_ALLOW_LOOPBACK_FETCH");
    }

    Ok(())
}

async fn spawn_test_server() -> Result<(String, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind test server")?;
    let addr: SocketAddr = listener.local_addr().context("failed to read test server addr")?;

    let app = Router::new().route(
        "/data",
        get(|| async {
            ([("content-length", "17")], "buffered-response")
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