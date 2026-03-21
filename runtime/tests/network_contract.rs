use std::sync::Once;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use rcgen::generate_simple_self_signed;
use runtime::JsIsolate;
use runtime::deno_runtime::{ExecutionMode, FetchCheckpoint};
use runtime::isolate_pool::ExecutionContext;
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};
use tokio_rustls::TlsAcceptor;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_exchange_replays_buffered_response() -> Result<()> {
    let _lock = network_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLUXBASE_ALLOW_LOOPBACK_TCP", "1");
    let (port, shutdown_tx, server_task) = spawn_tcp_server(b"PONG:ping", None).await?;

    let code = r#"
export default function handler({ input }) {
  const result = Flux.net.tcpExchange({
        host: input.host,
        port: input.port,
        data: new TextEncoder().encode(input.message),
  });

  return {
    text: result.text,
    replay: result.replay,
    bytes: Array.from(result.bytes),
  };
}
"#;

    let payload = serde_json::json!({
        "host": "127.0.0.1",
        "port": port,
        "message": "ping",
    });

    let mut isolate = JsIsolate::new_for_run(code).await.context("failed to create live isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("tcp-live"))
        .await
        .context("live tcp execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("tcp server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "text": "PONG:ping",
            "replay": false,
            "bytes": [80, 79, 78, 71, 58, 112, 105, 110, 103],
        })
    );
    assert_eq!(live_output.checkpoints.len(), 1);
    assert_eq!(live_output.checkpoints[0].boundary, "tcp");
    assert_eq!(live_output.checkpoints[0].method, "exchange");

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate =
        JsIsolate::new_for_run(code).await.context("failed to create replay isolate")?;
    let mut replay_context = ExecutionContext::new("tcp-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("replay tcp execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(
        replay_output.output,
        serde_json::json!({
            "text": "PONG:ping",
            "replay": true,
            "bytes": [80, 79, 78, 71, 58, 112, 105, 110, 103],
        })
    );
    assert_eq!(replay_output.checkpoints.len(), 1);
    assert_eq!(replay_output.checkpoints[0].boundary, "tcp");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_exchange_supports_fixed_read_mode() -> Result<()> {
    let _lock = network_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLUXBASE_ALLOW_LOOPBACK_TCP", "1");
    let (port, shutdown_tx, server_task) = spawn_tcp_server(b"PONG", Some(4)).await?;

    let code = r#"
export default function handler({ input }) {
  const result = Flux.net.tcpExchange({
        host: input.host,
        port: input.port,
    text: "ping",
    readMode: "fixed",
    readBytes: 4,
  });

  return {
    text: result.text,
    bytes: Array.from(result.bytes),
  };
}
"#;

    let mut isolate =
        JsIsolate::new_for_run(code).await.context("failed to create fixed-read isolate")?;
    let output = isolate
        .execute(
            serde_json::json!({ "host": "127.0.0.1", "port": port }),
            ExecutionContext::new("tcp-fixed"),
        )
        .await
        .context("fixed-read execution failed")?;

    shutdown_tx.send(()).ok();
    server_task
        .await
        .context("tcp fixed server task failed")??;

    assert_eq!(output.error, None);
    assert_eq!(
        output.output,
        serde_json::json!({
            "text": "PONG",
            "bytes": [80, 79, 78, 71],
        })
    );
    assert_eq!(output.checkpoints.len(), 1);
    assert_eq!(output.checkpoints[0].boundary, "tcp");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_exchange_supports_tls_with_custom_ca() -> Result<()> {
    let _lock = network_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLUXBASE_ALLOW_LOOPBACK_TCP", "1");
    ensure_rustls_provider();
    let cert = generate_simple_self_signed(vec!["localhost".to_string()])
        .context("failed to generate test certificate")?;
    let cert_pem = cert.cert.pem();
    let cert_der: CertificateDer<'static> = cert.cert.der().clone();
    let key_der = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()));

    let server_config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .context("failed to build TLS server config")?;

    let acceptor = TlsAcceptor::from(std::sync::Arc::new(server_config));
    let (port, shutdown_tx, server_task) = spawn_tls_server(acceptor, b"PONGTLS", 4).await?;

    let code = r#"
export default function handler({ input }) {
  const result = Flux.net.tcpExchange({
    host: input.host,
    port: input.port,
    text: "ping",
    readMode: "fixed",
    readBytes: 7,
    tls: true,
    serverName: "localhost",
    caCertPem: input.caCertPem,
  });

  return {
    text: result.text,
    replay: result.replay,
    bytes: Array.from(result.bytes),
  };
}
"#;

    let payload = serde_json::json!({
        "host": "127.0.0.1",
        "port": port,
        "caCertPem": cert_pem,
    });

    let mut isolate = JsIsolate::new_for_run(code).await.context("failed to create tls isolate")?;
    let output = isolate
        .execute(payload, ExecutionContext::new("tcp-tls-live"))
        .await
        .context("tls tcp execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("tls server task failed")??;

    assert_eq!(output.error, None);
    assert_eq!(
        output.output,
        serde_json::json!({
            "text": "PONGTLS",
            "replay": false,
            "bytes": [80, 79, 78, 71, 84, 76, 83],
        })
    );
    assert_eq!(output.checkpoints.len(), 1);
    assert_eq!(output.checkpoints[0].boundary, "tcp");
    assert_eq!(
        output.checkpoints[0].request.get("tls"),
        Some(&serde_json::json!(true))
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn tcp_exchange_blocks_loopback_by_default_and_never_replays_it() -> Result<()> {
    let _lock = network_test_lock().lock().await;
    let code = r#"
export default function handler({ input }) {
  try {
        const result = Flux.net.tcpExchange({ host: input.host, port: input.port, text: "ping" });
    return { ok: true, text: result.text };
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
        "host": "127.0.0.1",
        "port": 5432,
    });

    let mut live_isolate =
        JsIsolate::new_for_run(code).await.context("failed to create blocked isolate")?;
    let live_output = live_isolate
        .execute(payload.clone(), ExecutionContext::new("tcp-blocked-live"))
        .await
        .context("blocked tcp execution failed")?;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output.get("ok"),
        Some(&serde_json::json!(false))
    );
    let live_message = live_output
        .output
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    let live_string = live_output
        .output
        .get("string")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    assert!(
        live_message.contains("tcp connect blocked")
            || live_string.contains("tcp connect blocked")
            || live_message.contains("private/loopback")
            || live_string.contains("private/loopback")
    );
    assert!(live_output.checkpoints.is_empty());

    let fake_recording = vec![FetchCheckpoint {
        call_index: 0,
        boundary: "tcp".to_string(),
        url: "tcp://127.0.0.1:5432".to_string(),
        method: "exchange".to_string(),
        request: serde_json::json!({
            "host": "127.0.0.1",
            "port": 5432,
            "write_base64": "cGluZw==",
            "read_mode": "until_close",
            "read_bytes": null,
        }),
        response: serde_json::json!({
            "host": "127.0.0.1",
            "port": 5432,
            "response_base64": "UE9ORw==",
            "bytes_read": 4,
            "replay": true,
        }),
        duration_ms: 0,
    }];

    let mut replay_isolate =
        JsIsolate::new_for_run(code).await.context("failed to create blocked replay isolate")?;
    let mut replay_context = ExecutionContext::new("tcp-blocked-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, fake_recording)
        .await
        .context("blocked replay tcp execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(
        replay_output.output.get("ok"),
        Some(&serde_json::json!(false))
    );
    assert!(replay_output.checkpoints.is_empty());

    Ok(())
}

async fn spawn_tcp_server(
    response: &'static [u8],
    fixed_request_bytes: Option<usize>,
) -> Result<(
    u16,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<()>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind tcp test listener")?;
    let port = listener
        .local_addr()
        .context("failed to get tcp listener addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept tcp client")?;
                let mut request_bytes = Vec::new();
                match fixed_request_bytes {
                    Some(expected) => {
                        request_bytes.resize(expected, 0);
                        socket.read_exact(&mut request_bytes).await.context("failed to read fixed tcp request")?;
                    }
                    None => {
                        socket.read_to_end(&mut request_bytes).await.context("failed to read tcp request")?;
                    }
                }
                socket.write_all(response).await.context("failed to write tcp response")?;
                socket.shutdown().await.context("failed to shutdown tcp server socket")?;
                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

async fn spawn_tls_server(
    acceptor: TlsAcceptor,
    response: &'static [u8],
    fixed_request_bytes: usize,
) -> Result<(
    u16,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<()>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind tls test listener")?;
    let port = listener
        .local_addr()
        .context("failed to get tls listener addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (socket, _) = accepted.context("failed to accept tls client")?;
                let mut socket = acceptor.accept(socket).await.context("failed to complete tls handshake")?;
                let mut request_bytes = vec![0; fixed_request_bytes];
                socket.read_exact(&mut request_bytes).await.context("failed to read tls request")?;
                socket.write_all(response).await.context("failed to write tls response")?;
                socket.shutdown().await.context("failed to shutdown tls server socket")?;
                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

fn network_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn ensure_rustls_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

struct EnvVarGuard {
    key: &'static str,
    previous: Option<String>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let previous = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, value);
        }
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe {
                std::env::set_var(self.key, value);
            },
            None => unsafe {
                std::env::remove_var(self.key);
            },
        }
    }
}
