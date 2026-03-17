use std::sync::OnceLock;

use anyhow::{Context, Result};
use runtime::JsIsolate;
use runtime::deno_runtime::{ExecutionMode, FetchCheckpoint};
use runtime::isolate_pool::ExecutionContext;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{Mutex, oneshot};

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_simple_query_replays_recorded_rows() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_POSTGRES", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_postgres_server().await?;

    let code = r#"
export default function handler({ input }) {
  const result = Flux.postgres.simpleQuery({
    connectionString: input.connectionString,
    sql: input.sql,
  });

  return {
    rows: result.rows,
    command: result.command,
    replay: result.replay,
  };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("postgres://127.0.0.1:{port}/flux_test"),
        "sql": "select 1 as value",
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create postgres isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-live"))
        .await
        .context("live postgres execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("mock postgres server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "rows": [{ "value": "1" }],
            "command": "SELECT 1",
            "replay": false,
        })
    );
    assert_eq!(live_output.checkpoints.len(), 1);
    assert_eq!(live_output.checkpoints[0].boundary, "postgres");
    assert_eq!(live_output.checkpoints[0].method, "simple_query");

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create postgres replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("replay postgres execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(
        replay_output.output,
        serde_json::json!({
            "rows": [{ "value": "1" }],
            "command": "SELECT 1",
            "replay": true,
        })
    );
    assert_eq!(replay_output.checkpoints.len(), 1);
    assert_eq!(replay_output.checkpoints[0].boundary, "postgres");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_simple_query_blocks_loopback_by_default_and_never_replays_it() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let code = r#"
export default function handler({ input }) {
  try {
    const result = Flux.postgres.simpleQuery({
      connectionString: input.connectionString,
      sql: input.sql,
    });
    return { ok: true, rows: result.rows };
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
        "connectionString": "postgres://127.0.0.1:5432/flux_test",
        "sql": "select 1 as value",
    });

    let mut live_isolate = JsIsolate::new_for_run(code).context("failed to create blocked postgres isolate")?;
    let live_output = live_isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-blocked-live"))
        .await
        .context("blocked postgres execution failed")?;

    assert_eq!(live_output.error, None);
    assert_eq!(live_output.output.get("ok"), Some(&serde_json::json!(false)));
    let live_message = live_output.output.get("message").and_then(|value| value.as_str()).unwrap_or("");
    let live_string = live_output.output.get("string").and_then(|value| value.as_str()).unwrap_or("");
    assert!(
        live_message.contains("postgres connect blocked")
            || live_string.contains("postgres connect blocked")
            || live_message.contains("private/loopback")
            || live_string.contains("private/loopback")
    );
    assert!(live_output.checkpoints.is_empty());

    let fake_recording = vec![FetchCheckpoint {
        call_index: 0,
        boundary: "postgres".to_string(),
        url: "postgres://127.0.0.1:5432/flux_test".to_string(),
        method: "simple_query".to_string(),
        request: serde_json::json!({
            "url": "postgres://127.0.0.1:5432/flux_test",
            "host": "127.0.0.1",
            "port": 5432,
            "sql": "select 1 as value",
        }),
        response: serde_json::json!({
            "rows": [{ "value": "1" }],
            "command": "SELECT 1",
            "row_count": 1,
            "replay": true,
        }),
        duration_ms: 0,
    }];

    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create blocked postgres replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-blocked-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, fake_recording)
        .await
        .context("blocked postgres replay execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(replay_output.output.get("ok"), Some(&serde_json::json!(false)));
    assert!(replay_output.checkpoints.is_empty());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn postgres_query_supports_params_and_replay() -> Result<()> {
    let _lock = postgres_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_POSTGRES", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_postgres_param_server().await?;

    let code = r#"
export default function handler({ input }) {
  const result = Flux.postgres.query({
    connectionString: input.connectionString,
    sql: input.sql,
    params: input.params,
  });

  return {
    rows: result.rows,
    command: result.command,
    replay: result.replay,
  };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("postgres://127.0.0.1:{port}/flux_test"),
        "sql": "select $1::text as value",
        "params": ["hello"],
    });

    let mut isolate = JsIsolate::new_for_run(code).context("failed to create postgres param isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("postgres-param-live"))
        .await
        .context("live postgres param execution failed")?;

    shutdown_tx.send(()).ok();
    server_task.await.context("mock postgres param server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "rows": [{ "value": "hello" }],
            "command": "QUERY",
            "replay": false,
        })
    );
    assert_eq!(live_output.checkpoints.len(), 1);
    assert_eq!(live_output.checkpoints[0].boundary, "postgres");
    assert_eq!(live_output.checkpoints[0].method, "query");

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate = JsIsolate::new_for_run(code).context("failed to create postgres param replay isolate")?;
    let mut replay_context = ExecutionContext::new("postgres-param-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("replay postgres param execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(
        replay_output.output,
        serde_json::json!({
            "rows": [{ "value": "hello" }],
            "command": "QUERY",
            "replay": true,
        })
    );
    assert_eq!(replay_output.checkpoints.len(), 1);
    assert_eq!(replay_output.checkpoints[0].boundary, "postgres");

    Ok(())
}

async fn spawn_mock_postgres_server() -> Result<(u16, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock postgres listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock postgres addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept postgres client")?;

                let _startup = read_startup_message(&mut socket).await?;
                write_authentication_ok(&mut socket).await?;
                write_parameter_status(&mut socket, b"client_encoding", b"UTF8").await?;
                write_parameter_status(&mut socket, b"server_version", b"16.0").await?;
                write_backend_key_data(&mut socket).await?;
                write_ready_for_query(&mut socket).await?;

                let query = read_typed_message(&mut socket).await?;
                if query.tag != b'Q' {
                    anyhow::bail!("expected Query message, got {:?}", query.tag as char);
                }
                let sql = String::from_utf8(query.payload[..query.payload.len().saturating_sub(1)].to_vec())
                    .context("invalid query payload")?;
                if sql != "select 1 as value" {
                    anyhow::bail!("unexpected SQL: {sql}");
                }

                write_row_description(&mut socket, b"value").await?;
                write_data_row(&mut socket, &[b"1"] ).await?;
                write_command_complete(&mut socket, b"SELECT 1").await?;
                write_ready_for_query(&mut socket).await?;

                let terminate = read_typed_message(&mut socket).await?;
                if terminate.tag != b'X' {
                    anyhow::bail!("expected Terminate message, got {:?}", terminate.tag as char);
                }
                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

async fn spawn_mock_postgres_param_server() -> Result<(u16, oneshot::Sender<()>, tokio::task::JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock postgres param listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock postgres param addr")?
        .port();
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        tokio::select! {
            _ = &mut shutdown_rx => Ok(()),
            accepted = listener.accept() => {
                let (mut socket, _) = accepted.context("failed to accept postgres param client")?;

                let _startup = read_startup_message(&mut socket).await?;
                write_authentication_ok(&mut socket).await?;
                write_parameter_status(&mut socket, b"client_encoding", b"UTF8").await?;
                write_parameter_status(&mut socket, b"server_version", b"16.0").await?;
                write_backend_key_data(&mut socket).await?;
                write_ready_for_query(&mut socket).await?;

                let parse = read_typed_message(&mut socket).await?;
                if parse.tag != b'P' {
                    anyhow::bail!("expected Parse message, got {:?}", parse.tag as char);
                }
                let describe = read_typed_message(&mut socket).await?;
                if describe.tag != b'D' {
                    anyhow::bail!("expected Describe message, got {:?}", describe.tag as char);
                }
                let sync = read_typed_message(&mut socket).await?;
                if sync.tag != b'S' {
                    anyhow::bail!("expected Sync after Parse/Describe, got {:?}", sync.tag as char);
                }

                write_message(&mut socket, b'1', |_| {}).await?;
                write_parameter_description(&mut socket, &[25]).await?;
                write_row_description(&mut socket, b"value").await?;
                write_ready_for_query(&mut socket).await?;

                let bind = read_typed_message(&mut socket).await?;
                if bind.tag != b'B' {
                    anyhow::bail!("expected Bind message, got {:?}", bind.tag as char);
                }
                let param_value = parse_bind_first_text_param(&bind.payload)?;
                if param_value != "hello" {
                    anyhow::bail!("unexpected bound parameter: {param_value}");
                }

                let execute = read_typed_message(&mut socket).await?;
                if execute.tag != b'E' {
                    anyhow::bail!("expected Execute message, got {:?}", execute.tag as char);
                }
                let sync = read_typed_message(&mut socket).await?;
                if sync.tag != b'S' {
                    anyhow::bail!("expected Sync after Execute, got {:?}", sync.tag as char);
                }

                write_message(&mut socket, b'2', |_| {}).await?;
                write_data_row(&mut socket, &[b"hello"]).await?;
                write_command_complete(&mut socket, b"SELECT 1").await?;
                write_ready_for_query(&mut socket).await?;

                let next = read_typed_message(&mut socket).await?;
                if next.tag == b'C' {
                    let sync = read_typed_message(&mut socket).await?;
                    if sync.tag != b'S' {
                        anyhow::bail!("expected Sync after Close, got {:?}", sync.tag as char);
                    }
                    write_message(&mut socket, b'3', |_| {}).await?;
                    write_ready_for_query(&mut socket).await?;

                    let terminate = read_typed_message(&mut socket).await?;
                    if terminate.tag != b'X' {
                        anyhow::bail!("expected Terminate message, got {:?}", terminate.tag as char);
                    }
                } else if next.tag != b'X' {
                    anyhow::bail!("expected Close or Terminate message, got {:?}", next.tag as char);
                }
                Ok(())
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

struct TypedMessage {
    tag: u8,
    payload: Vec<u8>,
}

async fn read_startup_message(socket: &mut tokio::net::TcpStream) -> Result<Vec<u8>> {
    let length = socket.read_i32().await.context("failed to read startup length")? as usize;
    let mut payload = vec![0; length.saturating_sub(4)];
    socket.read_exact(&mut payload).await.context("failed to read startup payload")?;
    Ok(payload)
}

async fn read_typed_message(socket: &mut tokio::net::TcpStream) -> Result<TypedMessage> {
    let tag = socket.read_u8().await.context("failed to read message tag")?;
    let length = socket.read_i32().await.context("failed to read message length")? as usize;
    let mut payload = vec![0; length.saturating_sub(4)];
    socket.read_exact(&mut payload).await.context("failed to read message payload")?;
    Ok(TypedMessage { tag, payload })
}

async fn write_authentication_ok(socket: &mut tokio::net::TcpStream) -> Result<()> {
    write_message(socket, b'R', |buf| buf.extend_from_slice(&0u32.to_be_bytes())).await
}

async fn write_parameter_status(socket: &mut tokio::net::TcpStream, key: &[u8], value: &[u8]) -> Result<()> {
    write_message(socket, b'S', |buf| {
        buf.extend_from_slice(key);
        buf.push(0);
        buf.extend_from_slice(value);
        buf.push(0);
    })
    .await
}

async fn write_backend_key_data(socket: &mut tokio::net::TcpStream) -> Result<()> {
    write_message(socket, b'K', |buf| {
        buf.extend_from_slice(&1u32.to_be_bytes());
        buf.extend_from_slice(&2u32.to_be_bytes());
    })
    .await
}

async fn write_parameter_description(socket: &mut tokio::net::TcpStream, oids: &[u32]) -> Result<()> {
    write_message(socket, b't', |buf| {
        buf.extend_from_slice(&(oids.len() as u16).to_be_bytes());
        for oid in oids {
            buf.extend_from_slice(&oid.to_be_bytes());
        }
    })
    .await
}

async fn write_ready_for_query(socket: &mut tokio::net::TcpStream) -> Result<()> {
    write_message(socket, b'Z', |buf| buf.push(b'I')).await
}

async fn write_row_description(socket: &mut tokio::net::TcpStream, column_name: &[u8]) -> Result<()> {
    write_message(socket, b'T', |buf| {
        buf.extend_from_slice(&1u16.to_be_bytes());
        buf.extend_from_slice(column_name);
        buf.push(0);
        buf.extend_from_slice(&0u32.to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());
        buf.extend_from_slice(&25u32.to_be_bytes());
        buf.extend_from_slice(&(-1i16).to_be_bytes());
        buf.extend_from_slice(&(-1i32).to_be_bytes());
        buf.extend_from_slice(&0u16.to_be_bytes());
    })
    .await
}

async fn write_data_row(socket: &mut tokio::net::TcpStream, values: &[&[u8]]) -> Result<()> {
    write_message(socket, b'D', |buf| {
        buf.extend_from_slice(&(values.len() as u16).to_be_bytes());
        for value in values {
            buf.extend_from_slice(&(value.len() as i32).to_be_bytes());
            buf.extend_from_slice(value);
        }
    })
    .await
}

async fn write_command_complete(socket: &mut tokio::net::TcpStream, tag: &[u8]) -> Result<()> {
    write_message(socket, b'C', |buf| {
        buf.extend_from_slice(tag);
        buf.push(0);
    })
    .await
}

fn parse_bind_first_text_param(payload: &[u8]) -> Result<String> {
    let mut idx = 0usize;
    idx = skip_c_string(payload, idx)?;
    idx = skip_c_string(payload, idx)?;

    let format_count = read_u16(payload, &mut idx)? as usize;
    idx = idx.saturating_add(format_count * 2);

    let param_count = read_u16(payload, &mut idx)? as usize;
    if param_count == 0 {
        anyhow::bail!("bind payload contained no parameters");
    }

    let param_len = read_i32(payload, &mut idx)?;
    if param_len < 0 {
        anyhow::bail!("first bind parameter was null");
    }
    let len = param_len as usize;
    let end = idx.saturating_add(len);
    if end > payload.len() {
        anyhow::bail!("bind payload parameter truncated");
    }
    let value = String::from_utf8(payload[idx..end].to_vec()).context("invalid bind parameter utf8")?;
    Ok(value)
}

fn skip_c_string(payload: &[u8], mut idx: usize) -> Result<usize> {
    while idx < payload.len() {
        if payload[idx] == 0 {
            return Ok(idx + 1);
        }
        idx += 1;
    }
    anyhow::bail!("unterminated c-string in postgres payload")
}

fn read_u16(payload: &[u8], idx: &mut usize) -> Result<u16> {
    let end = idx.saturating_add(2);
    if end > payload.len() {
        anyhow::bail!("short postgres payload reading u16");
    }
    let value = u16::from_be_bytes([payload[*idx], payload[*idx + 1]]);
    *idx = end;
    Ok(value)
}

fn read_i32(payload: &[u8], idx: &mut usize) -> Result<i32> {
    let end = idx.saturating_add(4);
    if end > payload.len() {
        anyhow::bail!("short postgres payload reading i32");
    }
    let value = i32::from_be_bytes([
        payload[*idx],
        payload[*idx + 1],
        payload[*idx + 2],
        payload[*idx + 3],
    ]);
    *idx = end;
    Ok(value)
}

async fn write_message<F>(socket: &mut tokio::net::TcpStream, tag: u8, build: F) -> Result<()>
where
    F: FnOnce(&mut Vec<u8>),
{
    let mut payload = Vec::new();
    build(&mut payload);
    socket.write_u8(tag).await.context("failed to write message tag")?;
    socket
        .write_i32((payload.len() as i32) + 4)
        .await
        .context("failed to write message length")?;
    socket.write_all(&payload).await.context("failed to write message payload")?;
    Ok(())
}

fn postgres_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
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
