use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;

use anyhow::{Context, Result};
use runtime::JsIsolate;
use runtime::deno_runtime::{ExecutionMode, FetchCheckpoint};
use runtime::isolate_pool::ExecutionContext;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, oneshot};
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redis_command_replays_recorded_value() -> Result<()> {
    let _lock = redis_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_REDIS", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_redis_server().await?;

    {
        let mut seed = TcpStream::connect(("127.0.0.1", port))
            .await
            .context("failed to connect to mock redis seed socket")?;
        write_resp_command(&mut seed, &["SET", "greeting", "hello"]).await?;
        read_resp_value(&mut seed).await?;
    }

    let code = r#"
export default function handler({ input }) {
    const result = Flux.redis.command({
        connectionString: input.connectionString,
        command: "GET",
        args: ["greeting"],
    });

    return {
        value: result.value,
        replay: result.replay,
    };
}
"#;

    let payload = serde_json::json!({
        "connectionString": format!("redis://127.0.0.1:{port}/0"),
    });

    let mut isolate = JsIsolate::new_for_run(code).await.context("failed to create redis isolate")?;
    let live_output = isolate
        .execute(payload.clone(), ExecutionContext::new("redis-live"))
        .await
        .context("live redis execution failed")?;

    shutdown_tx.send(()).ok();
    server_task
        .await
        .context("mock redis server task failed")??;

    assert_eq!(live_output.error, None);
    assert_eq!(
        live_output.output,
        serde_json::json!({
            "value": "hello",
            "replay": false,
        })
    );
    assert_eq!(live_output.checkpoints.len(), 1);
    assert_eq!(live_output.checkpoints[0].boundary, "redis");
    assert_eq!(live_output.checkpoints[0].method, "GET");

    let recorded = live_output.checkpoints.clone();
    let mut replay_isolate =
        JsIsolate::new_for_run(code).await.context("failed to create redis replay isolate")?;
    let mut replay_context = ExecutionContext::new("redis-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, recorded)
        .await
        .context("redis replay execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(
        replay_output.output,
        serde_json::json!({
            "value": "hello",
            "replay": true,
        })
    );
    assert_eq!(replay_output.checkpoints.len(), 1);
    assert_eq!(replay_output.checkpoints[0].boundary, "redis");

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redis_blocks_loopback_by_default_and_never_replays_it() -> Result<()> {
    let _lock = redis_test_lock().lock().await;

    let code = r#"
export default function handler({ input }) {
    try {
        const result = Flux.redis.command({
            connectionString: input.connectionString,
            command: "GET",
            args: ["greeting"],
        });
        return { ok: true, value: result.value };
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
        "connectionString": "redis://127.0.0.1:6379/0",
    });

    let mut live_isolate =
        JsIsolate::new_for_run(code).await.context("failed to create blocked redis isolate")?;
    let live_output = live_isolate
        .execute(payload.clone(), ExecutionContext::new("redis-blocked-live"))
        .await
        .context("blocked redis execution failed")?;

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
        live_message.contains("redis connect blocked")
            || live_string.contains("redis connect blocked")
            || live_message.contains("private/loopback")
            || live_string.contains("private/loopback")
    );
    assert!(live_output.checkpoints.is_empty());

    let fake_recording = vec![FetchCheckpoint {
        call_index: 0,
        boundary: "redis".to_string(),
        url: "redis://127.0.0.1:6379/0".to_string(),
        method: "GET".to_string(),
        request: serde_json::json!({
            "url": "redis://127.0.0.1:6379/0",
            "host": "127.0.0.1",
            "port": 6379,
            "db": 0,
            "command": "GET",
            "args": ["greeting"],
        }),
        response: serde_json::json!({
            "value": "hello",
            "error": serde_json::Value::Null,
            "replay": false,
        }),
        duration_ms: 0,
    }];

    let mut replay_isolate =
        JsIsolate::new_for_run(code).await.context("failed to create blocked redis replay isolate")?;
    let mut replay_context = ExecutionContext::new("redis-blocked-replay");
    replay_context.mode = ExecutionMode::Replay;
    let replay_output = replay_isolate
        .execute_with_recorded(payload, replay_context, fake_recording)
        .await
        .context("blocked redis replay execution failed")?;

    assert_eq!(replay_output.error, None);
    assert_eq!(
        replay_output.output.get("ok"),
        Some(&serde_json::json!(false))
    );
    assert!(replay_output.checkpoints.is_empty());

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redis_module_shim_supports_common_commands() -> Result<()> {
    let _lock = redis_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_REDIS", "1");
    let (port, shutdown_tx, server_task) = spawn_mock_redis_server().await?;

    let code = r#"
import { createClient } from "redis";

export default async function handler({ input }) {
    const client = createClient({ url: input.connectionString });
    await client.connect();
    await client.set("count", "1");
    const exists = await client.exists("count");
    const incremented = await client.incr("count");
    const decremented = await client.decr("count");
    const value = await client.get("count");
    await client.hSet("profile", "name", "flux");
    const field = await client.hGet("profile", "name");
    const expired = await client.expire("profile", 60);
    const ttl = await client.ttl("profile");
    const hashDeleted = await client.hDel("profile", "name");
    const echoed = await client.sendCommand(["GET", "count"]);
    const deleted = await client.del("count");
    let blocked = null;
    try {
        client.multi();
    } catch (error) {
        blocked = String(error?.message ?? error);
    }
    await client.quit();
    await client.disconnect();

    return {
        value,
        echoed,
        exists,
        incremented,
        decremented,
        field,
        expired,
        ttl,
        hashDeleted,
        deleted,
        blocked,
        isClient: client instanceof Flux.redis.FluxRedisClient,
    };
}
"#;

    let temp_module = TempModule::new(code)?;

    let payload = serde_json::json!({
        "connectionString": format!("redis://127.0.0.1:{port}/0"),
    });

    let mut isolate = JsIsolate::new_for_run_entry(temp_module.entry())
        .await
        .context("failed to create redis shim isolate")?;
    let output = isolate
        .execute(payload, ExecutionContext::new("redis-shim-live"))
        .await
        .context("redis shim execution failed")?;

    shutdown_tx.send(()).ok();
    server_task
        .await
        .context("mock redis shim server task failed")??;

    assert_eq!(output.error, None);
    assert_eq!(
        output.output,
        serde_json::json!({
            "value": "1",
            "echoed": "1",
            "exists": 1,
            "incremented": 2,
            "decremented": 1,
            "field": "flux",
            "expired": 1,
            "ttl": 60,
            "hashDeleted": 1,
            "deleted": 1,
            "blocked": "Redis transactions are not supported in Flux (non-deterministic execution)",
            "isClient": true,
        })
    );
    assert_eq!(output.checkpoints.len(), 12);
    assert!(
        output
            .checkpoints
            .iter()
            .all(|checkpoint| checkpoint.boundary == "redis")
    );

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn redis_boundary_blocks_nondeterministic_commands() -> Result<()> {
    let _lock = redis_test_lock().lock().await;
    let _guard = EnvVarGuard::set("FLOWBASE_ALLOW_LOOPBACK_REDIS", "1");

    let code = r#"
export default function handler({ input }) {
    try {
        Flux.redis.command({
            connectionString: input.connectionString,
            command: "BLPOP",
            args: ["jobs", "0"],
        });
        return { ok: true };
    } catch (error) {
        return {
            ok: false,
            message: String(error?.message ?? error),
        };
    }
}
"#;

    let payload = serde_json::json!({
        "connectionString": "redis://127.0.0.1:6379/0",
    });

    let mut isolate =
        JsIsolate::new_for_run(code).await.context("failed to create blocked-command isolate")?;
    let output = isolate
        .execute(payload, ExecutionContext::new("redis-blocked-command"))
        .await
        .context("blocked command execution failed")?;

    assert_eq!(output.error, None);
    assert_eq!(
        output.output,
        serde_json::json!({
            "ok": false,
            "message": "Redis blocking commands are not supported in Flux (non-deterministic execution)",
        })
    );
    assert!(output.checkpoints.is_empty());

    Ok(())
}

#[derive(Default)]
struct RedisState {
    strings: HashMap<String, String>,
    hashes: HashMap<String, HashMap<String, String>>,
    expirations: HashMap<String, i64>,
}

async fn spawn_mock_redis_server() -> Result<(
    u16,
    oneshot::Sender<()>,
    tokio::task::JoinHandle<Result<()>>,
)> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .context("failed to bind mock redis listener")?;
    let port = listener
        .local_addr()
        .context("failed to get mock redis addr")?
        .port();
    let state = Arc::new(Mutex::new(RedisState::default()));
    let (shutdown_tx, mut shutdown_rx) = oneshot::channel();

    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut shutdown_rx => break Ok(()),
                accepted = listener.accept() => {
                    let (mut socket, _) = accepted.context("failed to accept redis client")?;
                    let state = Arc::clone(&state);
                    tokio::spawn(async move {
                        let _ = handle_redis_connection(&mut socket, state).await;
                    });
                }
            }
        }
    });

    Ok((port, shutdown_tx, task))
}

async fn handle_redis_connection(
    socket: &mut TcpStream,
    state: Arc<Mutex<RedisState>>,
) -> Result<()> {
    loop {
        let command = match read_resp_command(socket).await {
            Ok(command) => command,
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error).context("failed to read redis command"),
        };

        if command.is_empty() {
            write_error(socket, "ERR empty command").await?;
            continue;
        }

        let name = command[0].to_ascii_uppercase();
        match name.as_str() {
            "AUTH" | "SELECT" => write_simple_string(socket, "OK").await?,
            "GET" => {
                let key = command.get(1).cloned().unwrap_or_default();
                let value = state.lock().await.strings.get(&key).cloned();
                write_bulk_string(socket, value.as_deref()).await?;
            }
            "SET" => {
                let key = command.get(1).cloned().unwrap_or_default();
                let value = command.get(2).cloned().unwrap_or_default();
                state.lock().await.strings.insert(key, value);
                write_simple_string(socket, "OK").await?;
            }
            "DEL" => {
                let mut deleted = 0i64;
                let mut guard = state.lock().await;
                for key in command.iter().skip(1) {
                    if guard.strings.remove(key).is_some() || guard.hashes.remove(key).is_some() {
                        guard.expirations.remove(key);
                        deleted += 1;
                    }
                }
                write_integer(socket, deleted).await?;
            }
            "EXISTS" => {
                let key = command.get(1).cloned().unwrap_or_default();
                let guard = state.lock().await;
                let exists = guard.strings.contains_key(&key) || guard.hashes.contains_key(&key);
                write_integer(socket, if exists { 1 } else { 0 }).await?;
            }
            "INCR" => {
                let key = command.get(1).cloned().unwrap_or_default();
                let mut guard = state.lock().await;
                let next = guard
                    .strings
                    .get(&key)
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0)
                    + 1;
                guard.strings.insert(key, next.to_string());
                write_integer(socket, next).await?;
            }
            "DECR" => {
                let key = command.get(1).cloned().unwrap_or_default();
                let mut guard = state.lock().await;
                let next = guard
                    .strings
                    .get(&key)
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0)
                    - 1;
                guard.strings.insert(key, next.to_string());
                write_integer(socket, next).await?;
            }
            "HGET" => {
                let key = command.get(1).cloned().unwrap_or_default();
                let field = command.get(2).cloned().unwrap_or_default();
                let value = state
                    .lock()
                    .await
                    .hashes
                    .get(&key)
                    .and_then(|hash| hash.get(&field))
                    .cloned();
                write_bulk_string(socket, value.as_deref()).await?;
            }
            "HSET" => {
                let key = command.get(1).cloned().unwrap_or_default();
                let field = command.get(2).cloned().unwrap_or_default();
                let value = command.get(3).cloned().unwrap_or_default();
                let mut guard = state.lock().await;
                let hash = guard.hashes.entry(key).or_default();
                let inserted = if hash.insert(field, value).is_some() {
                    0
                } else {
                    1
                };
                write_integer(socket, inserted).await?;
            }
            "HDEL" => {
                let key = command.get(1).cloned().unwrap_or_default();
                let field = command.get(2).cloned().unwrap_or_default();
                let mut guard = state.lock().await;
                let deleted = guard
                    .hashes
                    .get_mut(&key)
                    .and_then(|hash| hash.remove(&field))
                    .map(|_| 1)
                    .unwrap_or(0);
                if guard
                    .hashes
                    .get(&key)
                    .map(|hash| hash.is_empty())
                    .unwrap_or(false)
                {
                    guard.hashes.remove(&key);
                    guard.expirations.remove(&key);
                }
                write_integer(socket, deleted).await?;
            }
            "EXPIRE" => {
                let key = command.get(1).cloned().unwrap_or_default();
                let seconds = command
                    .get(2)
                    .and_then(|value| value.parse::<i64>().ok())
                    .unwrap_or(0);
                let mut guard = state.lock().await;
                let exists = guard.strings.contains_key(&key) || guard.hashes.contains_key(&key);
                if exists {
                    guard.expirations.insert(key, seconds);
                    write_integer(socket, 1).await?;
                } else {
                    write_integer(socket, 0).await?;
                }
            }
            "TTL" => {
                let key = command.get(1).cloned().unwrap_or_default();
                let guard = state.lock().await;
                let ttl = if let Some(seconds) = guard.expirations.get(&key) {
                    *seconds
                } else if guard.strings.contains_key(&key) || guard.hashes.contains_key(&key) {
                    -1
                } else {
                    -2
                };
                write_integer(socket, ttl).await?;
            }
            _ => write_error(socket, "ERR unknown command").await?,
        }
    }
}

async fn read_resp_command(socket: &mut TcpStream) -> std::io::Result<Vec<String>> {
    let prefix = read_byte(socket).await?;
    if prefix != b'*' {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "expected RESP array",
        ));
    }

    let count = read_resp_line(socket)
        .await
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))?
        .parse::<usize>()
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))?;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let prefix = read_byte(socket).await?;
        if prefix != b'$' {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "expected RESP bulk string",
            ));
        }
        let length = read_resp_line(socket)
            .await
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))?
            .parse::<usize>()
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))?;
        let mut bytes = vec![0u8; length];
        socket.read_exact(&mut bytes).await?;
        consume_resp_crlf(socket)
            .await
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))?;
        values.push(String::from_utf8_lossy(&bytes).into_owned());
    }

    Ok(values)
}

async fn read_resp_value(socket: &mut TcpStream) -> Result<()> {
    let prefix = read_byte(socket)
        .await
        .context("failed to read RESP prefix")?;
    match prefix {
        b'+' | b'-' => {
            let _ = read_resp_line(socket).await?;
            Ok(())
        }
        b':' => {
            let _ = read_resp_line(socket).await?.parse::<i64>()?;
            Ok(())
        }
        b'$' => {
            let length = read_resp_line(socket).await?.parse::<isize>()?;
            if length < 0 {
                return Ok(());
            }
            let mut bytes = vec![0u8; length as usize];
            socket.read_exact(&mut bytes).await?;
            consume_resp_crlf(socket).await?;
            Ok(())
        }
        other => anyhow::bail!("unsupported RESP prefix: {:?}", other as char),
    }
}

async fn write_resp_command(socket: &mut TcpStream, parts: &[&str]) -> Result<()> {
    let mut payload = Vec::new();
    payload.extend_from_slice(format!("*{}\r\n", parts.len()).as_bytes());
    for part in parts {
        payload.extend_from_slice(format!("${}\r\n", part.len()).as_bytes());
        payload.extend_from_slice(part.as_bytes());
        payload.extend_from_slice(b"\r\n");
    }
    socket.write_all(&payload).await?;
    socket.flush().await?;
    Ok(())
}

async fn write_simple_string(socket: &mut TcpStream, value: &str) -> Result<()> {
    socket
        .write_all(format!("+{}\r\n", value).as_bytes())
        .await?;
    socket.flush().await?;
    Ok(())
}

async fn write_bulk_string(socket: &mut TcpStream, value: Option<&str>) -> Result<()> {
    match value {
        Some(value) => {
            socket
                .write_all(format!("${}\r\n{}\r\n", value.len(), value).as_bytes())
                .await?;
        }
        None => {
            socket.write_all(b"$-1\r\n").await?;
        }
    }
    socket.flush().await?;
    Ok(())
}

async fn write_integer(socket: &mut TcpStream, value: i64) -> Result<()> {
    socket
        .write_all(format!(":{}\r\n", value).as_bytes())
        .await?;
    socket.flush().await?;
    Ok(())
}

async fn write_error(socket: &mut TcpStream, value: &str) -> Result<()> {
    socket
        .write_all(format!("-{}\r\n", value).as_bytes())
        .await?;
    socket.flush().await?;
    Ok(())
}

async fn read_resp_line(socket: &mut TcpStream) -> Result<String> {
    let mut bytes = Vec::new();
    loop {
        let next = read_byte(socket).await?;
        if next == b'\r' {
            let lf = read_byte(socket).await?;
            if lf != b'\n' {
                anyhow::bail!("invalid RESP line ending");
            }
            break;
        }
        bytes.push(next);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn read_byte(socket: &mut TcpStream) -> std::io::Result<u8> {
    let mut byte = [0u8; 1];
    socket.read_exact(&mut byte).await?;
    Ok(byte[0])
}

async fn consume_resp_crlf(socket: &mut TcpStream) -> Result<()> {
    let mut suffix = [0u8; 2];
    socket.read_exact(&mut suffix).await?;
    if suffix != [b'\r', b'\n'] {
        anyhow::bail!("invalid RESP bulk terminator");
    }
    Ok(())
}

fn redis_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
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

struct TempModule {
    dir: PathBuf,
    entry: PathBuf,
}

impl TempModule {
    fn new(source: &str) -> Result<Self> {
        let dir = std::env::temp_dir().join(format!("flux-redis-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create temp dir {}", dir.display()))?;
        let entry = dir.join("index.ts");
        std::fs::write(&entry, source)
            .with_context(|| format!("failed to write temp module {}", entry.display()))?;
        Ok(Self { dir, entry })
    }

    fn entry(&self) -> &Path {
        &self.entry
    }
}

impl Drop for TempModule {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
