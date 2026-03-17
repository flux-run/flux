use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Once;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::{Context, Result};
use base64::Engine;
use deno_ast::{EmitOptions, MediaType, ParseParams, SourceMapOption, TranspileModuleOptions, TranspileOptions};
use deno_core::{JsRuntime, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader, ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, OpState, ResolutionKind, RuntimeOptions, op2, resolve_import, resolve_path};
use deno_error::JsErrorBox;
use postgres::config::SslMode as PostgresSslMode;
use postgres::{Client as PostgresClient, Config as PostgresConfig, NoTls, SimpleQueryMessage};
use postgres::types::ToSql;
use postgres_rustls::MakeTlsConnector as PostgresMakeTlsConnector;
use rand::Rng;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde::{Deserialize, Serialize};
use shared::project::{ArtifactMediaType, ArtifactModule, FluxBuildArtifact};
use tokio_rustls::TlsConnector;
use ureq::OrAnyStatus;
use url::Url;
use uuid::Uuid;

use crate::isolate_pool::ExecutionContext;

/// Per-isolate map of in-flight execution states, keyed by execution_id.
/// Stored once in `OpState`; each concurrent execution owns its own slot.
type RuntimeStateMap = HashMap<String, RuntimeExecutionState>;

/// Maximum response body size: 10 MB.
const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;

/// Maximum redirect hops for a single buffered fetch.
const MAX_REDIRECTS: usize = 5;

/// Blocked metadata hostnames (cloud provider instance metadata endpoints).
const BLOCKED_HOSTS: &[&str] = &[
    "169.254.169.254",
    "metadata.google.internal",
    "169.254.170.2",
];

const DEFAULT_CONNECT_TIMEOUT_MS: u64 = 5_000;
const DEFAULT_READ_TIMEOUT_MS: u64 = 5_000;

/// Validate that a URL is safe to fetch — blocks SSRF to cloud metadata and private IPs.
fn validate_fetch_url(raw_url: &str) -> std::result::Result<(), JsErrorBox> {
    let parsed = url::Url::parse(raw_url)
        .map_err(|e| JsErrorBox::type_error(format!("invalid URL: {e}")))?;

    let host = parsed
        .host_str()
        .ok_or_else(|| JsErrorBox::type_error("invalid URL: no host"))?;

    for blocked in BLOCKED_HOSTS {
        if host == *blocked {
            return Err(JsErrorBox::generic(format!(
                "fetch blocked: {host} is a restricted endpoint"
            )));
        }
    }

    let allow_loopback = std::env::var("FLOWBASE_ALLOW_LOOPBACK_FETCH")
        .map(|value| value == "1")
        .unwrap_or(false);

    if let Ok(ip) = host.parse::<IpAddr>() {
        validate_ip_addr(&ip, allow_loopback)?;
    } else if let Some(port) = parsed.port_or_known_default() {
        if let Ok(resolved) = (host, port).to_socket_addrs() {
            for addr in resolved {
                validate_ip_addr(&addr.ip(), allow_loopback)?;
            }
        }
    }

    Ok(())
}

fn validate_outbound_host(
    host: &str,
    port: u16,
    blocked_label: &str,
    allow_env_var: &str,
) -> std::result::Result<(), JsErrorBox> {
    for blocked in BLOCKED_HOSTS {
        if host == *blocked {
            return Err(JsErrorBox::generic(format!(
                "{blocked_label} blocked: {host} is a restricted endpoint"
            )));
        }
    }

    let allow_loopback = std::env::var(allow_env_var)
        .map(|value| value == "1")
        .unwrap_or(false);

    if let Ok(ip) = host.parse::<IpAddr>() {
        validate_ip_addr(&ip, allow_loopback).map_err(|_| {
            JsErrorBox::new(
                "Error",
                format!("{blocked_label} blocked: private/loopback IP addresses are not allowed"),
            )
        })?;
        return Ok(());
    }

    if let Ok(resolved) = (host, port).to_socket_addrs() {
        for addr in resolved {
            validate_ip_addr(&addr.ip(), allow_loopback).map_err(|_| {
                JsErrorBox::new(
                    "Error",
                    format!("{blocked_label} blocked: private/loopback IP addresses are not allowed"),
                )
            })?;
        }
    }

    Ok(())
}

fn validate_ip_addr(ip: &IpAddr, allow_loopback: bool) -> std::result::Result<(), JsErrorBox> {
    if ip.is_loopback() {
        if allow_loopback {
            return Ok(());
        }
        return Err(JsErrorBox::new(
            "Error",
            "fetch blocked: private/loopback IP addresses are not allowed",
        ));
    }

    if is_private_ip(ip) {
        return Err(JsErrorBox::new(
            "Error",
            "fetch blocked: private/loopback IP addresses are not allowed",
        ));
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
            (v6.segments()[0] & 0xfe00) == 0xfc00  // fc00::/7 unique-local
            || (v6.segments()[0] & 0xffc0) == 0xfe80  // fe80::/10 link-local
        }
    }
}

fn normalized_headers_value(headers: &HashMap<String, String>) -> serde_json::Value {
    let normalized: BTreeMap<String, String> = headers
        .iter()
        .map(|(key, value)| (key.to_ascii_lowercase(), value.clone()))
        .collect();
    serde_json::to_value(normalized).unwrap_or_else(|_| serde_json::json!({}))
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FetchRequestPayload {
    execution_id: String,
    url: String,
    method: String,
    body: Option<String>,
    headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Deserialize)]
struct TcpExchangeRequestPayload {
    execution_id: String,
    host: String,
    port: u16,
    #[serde(default)]
    write_bytes: Vec<u8>,
    #[serde(default = "default_tcp_read_mode")]
    read_mode: String,
    read_bytes: Option<usize>,
    connect_timeout_ms: Option<u64>,
    read_timeout_ms: Option<u64>,
    #[serde(default)]
    tls: bool,
    server_name: Option<String>,
    ca_cert_pem: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PostgresQueryRequestPayload {
    execution_id: String,
    connection_string: String,
    sql: String,
    #[serde(default)]
    params: Vec<serde_json::Value>,
    #[serde(default)]
    tls: bool,
    ca_cert_pem: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PostgresConnectRequestPayload {
    execution_id: String,
    connection_string: String,
    #[serde(default)]
    tls: bool,
    ca_cert_pem: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PostgresSessionQueryRequestPayload {
    execution_id: String,
    session_id: String,
    sql: String,
    #[serde(default)]
    params: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
struct PostgresSessionCloseRequestPayload {
    execution_id: String,
    session_id: String,
}

#[derive(Debug, Clone)]
struct PostgresTlsOptions {
    enabled: bool,
    ca_cert_pem: Option<String>,
}

#[derive(Debug, Clone)]
struct PostgresConnectionTarget {
    url: String,
    host: String,
    port: u16,
}

enum PostgresSessionCommand {
    Query {
        sql: String,
        params: Vec<serde_json::Value>,
        reply: mpsc::Sender<std::result::Result<PostgresSimpleQueryResponse, String>>,
    },
    Close,
}

struct PostgresSessionHandle {
    sender: mpsc::Sender<PostgresSessionCommand>,
    thread: Option<JoinHandle<()>>,
    target: PostgresConnectionTarget,
    tls: bool,
}

impl std::fmt::Debug for PostgresSessionHandle {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        fmt.debug_struct("PostgresSessionHandle")
            .field("target", &self.target)
            .field("tls", &self.tls)
            .finish()
    }
}

impl PostgresSessionHandle {
    fn connect(
        connection_string: &str,
        tls: &PostgresTlsOptions,
        target: PostgresConnectionTarget,
    ) -> Result<Self, JsErrorBox> {
        let (sender, receiver) = mpsc::channel::<PostgresSessionCommand>();
        let (ready_tx, ready_rx) = mpsc::channel::<std::result::Result<(), String>>();
        let connection_string = connection_string.to_string();
        let tls_options = tls.clone();
        let tls_enabled = tls.enabled;

        let thread = std::thread::spawn(move || {
            let mut client = match connect_postgres_client(&connection_string, &tls_options) {
                Ok(client) => {
                    let _ = ready_tx.send(Ok(()));
                    client
                }
                Err(err) => {
                    let _ = ready_tx.send(Err(err.to_string()));
                    return;
                }
            };

            while let Ok(command) = receiver.recv() {
                match command {
                    PostgresSessionCommand::Query { sql, params, reply } => {
                        let result = perform_postgres_query_with_client(&mut client, &sql, &params);
                        let _ = reply.send(result);
                    }
                    PostgresSessionCommand::Close => break,
                }
            }
        });

        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Self {
                sender,
                thread: Some(thread),
                target,
                tls: tls_enabled,
            }),
            Ok(Err(err)) => {
                let _ = thread.join();
                Err(JsErrorBox::type_error(err))
            }
            Err(_) => {
                let _ = thread.join();
                Err(JsErrorBox::generic("postgres connect failed: session worker exited before initialization"))
            }
        }
    }

    fn query(
        &self,
        sql: &str,
        params: &[serde_json::Value],
    ) -> Result<PostgresSimpleQueryResponse, JsErrorBox> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(PostgresSessionCommand::Query {
                sql: sql.to_string(),
                params: params.to_vec(),
                reply: reply_tx,
            })
            .map_err(|_| JsErrorBox::generic("postgres session is closed"))?;

        match reply_rx.recv() {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(err)) => Err(JsErrorBox::type_error(err)),
            Err(_) => Err(JsErrorBox::generic("postgres session worker exited unexpectedly")),
        }
    }

    fn shutdown(&mut self) {
        if let Some(thread) = self.thread.take() {
            let _ = self.sender.send(PostgresSessionCommand::Close);
            let _ = thread.join();
        }
    }
}

impl Drop for PostgresSessionHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn default_tcp_read_mode() -> String {
    "until_close".to_string()
}

/// A virtual HTTP request fed into a server-mode isolate by the Rust host.
#[derive(Debug, Clone)]
pub struct NetRequest {
    pub req_id: String,
    pub method: String,
    pub url: String,
    /// JSON-encoded `[[name, value], ...]` header pairs.
    pub headers_json: String,
    pub body: String,
}

/// The response produced by the JS handler and captured via `op_net_respond`.
#[derive(Debug, Clone)]
pub struct NetResponse {
    pub status: u16,
    /// `(name, value)` header pairs.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct NetRequestExecution {
    pub response: NetResponse,
    pub checkpoints: Vec<FetchCheckpoint>,
    pub logs: Vec<LogEntry>,
}

#[derive(Debug, Clone)]
pub struct JsExecutionOutput {
    pub output: serde_json::Value,
    pub checkpoints: Vec<FetchCheckpoint>,
    pub error: Option<String>,
    pub logs: Vec<LogEntry>,
}

#[derive(Debug)]
struct RuntimeExecutionState {
    context: ExecutionContext,
    call_index: u32,
    checkpoints: Vec<FetchCheckpoint>,
    /// Pre-recorded checkpoints for Replay mode, keyed by call_index.
    recorded: HashMap<u32, FetchCheckpoint>,
    /// First `Date.now()` seen in Live mode; returned verbatim in Replay mode.
    recorded_now_ms: Option<u64>,
    /// Console output captured during this execution.
    logs: Vec<LogEntry>,
    /// Random f64 values produced in Live mode; replayed in order in Replay mode.
    recorded_random: Vec<f64>,
    /// How many recorded_random values have been consumed so far in Replay mode.
    random_index: usize,
    /// UUID strings produced in Live mode; replayed in order in Replay mode.
    recorded_uuids: Vec<String>,
    /// How many recorded_uuids have been consumed so far in Replay mode.
    uuid_index: usize,
    /// Set to true when the user module calls `Deno.serve()`.
    is_server_mode: bool,
    /// Pending responses keyed by req_id, filled by `op_net_respond`.
    pending_responses: HashMap<String, NetResponse>,
    /// Live stateful Postgres sessions keyed by deterministic session id.
    postgres_sessions: HashMap<String, PostgresSessionHandle>,
    /// Next deterministic Postgres session id for this execution.
    next_postgres_session_id: u32,
}

deno_core::extension!(flux_runtime_ext, ops = [
    op_begin_execution,
    op_end_execution,
    op_flux_fetch,
    op_flux_tcp_exchange,
    op_flux_postgres_connect,
    op_flux_postgres_close_session,
    op_flux_postgres_simple_query,
    op_flux_postgres_query,
    op_flux_postgres_session_query,
    op_flux_now,
    op_flux_parse_url,
    op_console,
    op_timer_delay,
    op_random,
    op_random_uuid,
    op_net_listen,
    op_net_respond,
]);

/// Called by JS at the start of every execution to register a state slot.
/// `recorded_random_json` and `recorded_uuids_json` are JSON-encoded arrays for
/// replay mode; pass `"[]"` for live executions.
#[op2(fast)]
fn op_begin_execution(
    state: &mut OpState,
    #[string] execution_id: String,
    #[string] request_id: String,
    #[string] code_version: String,
    is_replay: bool,
    #[string] recorded_random_json: String,
    #[string] recorded_uuids_json: String,
    #[string] recorded_now_ms_json: String,
) {
    let recorded_random: Vec<f64> =
        serde_json::from_str(&recorded_random_json).unwrap_or_default();
    let recorded_uuids: Vec<String> =
        serde_json::from_str(&recorded_uuids_json).unwrap_or_default();
    let recorded_now_ms: Option<u64> =
        serde_json::from_str(&recorded_now_ms_json).unwrap_or(None);

    let exec_state = RuntimeExecutionState {
        context: ExecutionContext {
            execution_id: execution_id.clone(),
            request_id,
            code_version,
            mode: if is_replay { ExecutionMode::Replay } else { ExecutionMode::Live },
        },
        call_index: 0,
        checkpoints: Vec::new(),
        recorded: HashMap::new(),
        recorded_now_ms,
        logs: Vec::new(),
        recorded_random,
        random_index: 0,
        recorded_uuids,
        uuid_index: 0,
        is_server_mode: false,
        pending_responses: HashMap::new(),
        postgres_sessions: HashMap::new(),
        next_postgres_session_id: 0,
    };

    state
        .borrow_mut::<RuntimeStateMap>()
        .insert(execution_id, exec_state);
}

/// Called by JS at the end of every execution.  Returns a JSON string with the
/// collected checkpoints, logs, random values, and uuids so Rust can harvest
/// them without an extra op round-trip.
#[op2]
#[string]
fn op_end_execution(state: &mut OpState, #[string] execution_id: String) -> String {
    let slot = state
        .borrow_mut::<RuntimeStateMap>()
        .remove(&execution_id);

    match slot {
        Some(s) => serde_json::to_string(&serde_json::json!({
            "checkpoints": s.checkpoints,
            "logs":        s.logs,
            "random":      s.recorded_random,
            "uuids":       s.recorded_uuids,
            "now_ms":      s.recorded_now_ms,
        }))
        .unwrap_or_else(|_| "{}".to_string()),
        None => "{}".to_string(),
    }
}

#[op2]
#[serde]
fn op_flux_fetch(
    state: &mut OpState,
    #[serde] request: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let FetchRequestPayload {
        execution_id,
        url: original_url,
        method,
        body,
        headers,
    } = serde_json::from_value(request)
        .map_err(|err| JsErrorBox::type_error(format!("invalid fetch request: {err}")))?;

    let (request_id, call_index, mode, recorded_checkpoint) = {
        let (request_id, index, mode, recorded) = {
            let map = state.borrow_mut::<RuntimeStateMap>();
            let execution = map.get_mut(&execution_id).ok_or_else(|| {
                JsErrorBox::new(
                    "InternalError",
                    format!("op_flux_fetch: execution_id '{execution_id}' not found"),
                )
            })?;
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
        (request_id, index, mode, recorded)
    };

    // SSRF protection applies to live and replay paths. Recorded checkpoints must
    // never bypass the runtime's URL safety gate.
    validate_fetch_url(&original_url)?;

    // In Replay mode, return the recorded response instead of making a live call.
    if matches!(mode, ExecutionMode::Replay) {
        if let Some(checkpoint) = recorded_checkpoint {
            let response = checkpoint.response.clone();
            {
                let map = state.borrow_mut::<RuntimeStateMap>();
                if let Some(execution) = map.get_mut(&execution_id) {
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
            }
            tracing::debug!(%request_id, %call_index, "replay: returned recorded response");
            return Ok(response);
        }
        tracing::warn!(%request_id, %call_index, "replay: no recorded checkpoint, making live call");
    }

    let resolved_url = original_url.clone();
    let request_json = serde_json::json!({
        "url": original_url.clone(),
        "resolved_url": resolved_url.clone(),
        "method": method.clone(),
        "body": body.clone(),
        "headers": normalized_headers_value(&headers),
    });

    let started = std::time::Instant::now();
    let target_url = resolved_url;

    let response = make_http_request(&target_url, &method, body.clone(), Some(headers.clone()))?;
    let duration_ms = started.elapsed().as_millis() as i32;

    {
        let map = state.borrow_mut::<RuntimeStateMap>();
        if let Some(execution) = map.get_mut(&execution_id) {
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
    }

    tracing::debug!(%request_id, %call_index, original_url = %original_url, resolved_url = %target_url, "intercepted fetch");
    Ok(response)
}

#[op2]
#[serde]
fn op_flux_tcp_exchange(
    state: &mut OpState,
    #[serde] request: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let TcpExchangeRequestPayload {
        execution_id,
        host,
        port,
        write_bytes,
        read_mode,
        read_bytes,
        connect_timeout_ms,
        read_timeout_ms,
        tls,
        server_name,
        ca_cert_pem,
    } = serde_json::from_value(request)
        .map_err(|err| JsErrorBox::type_error(format!("invalid tcp exchange request: {err}")))?;

    let (request_id, call_index, mode, recorded_checkpoint) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "InternalError",
                format!("op_flux_tcp_exchange: execution_id '{execution_id}' not found"),
            )
        })?;
        let idx = execution.call_index;
        execution.call_index = execution.call_index.saturating_add(1);
        let recorded = execution.recorded.remove(&idx);
        (
            execution.context.request_id.clone(),
            idx,
            execution.context.mode.clone(),
            recorded,
        )
    };

    validate_outbound_host(&host, port, "tcp connect", "FLOWBASE_ALLOW_LOOPBACK_TCP")?;

    if matches!(mode, ExecutionMode::Replay) {
        if let Some(checkpoint) = recorded_checkpoint {
            let response = checkpoint.response.clone();
            let bytes = decode_checkpoint_bytes(&response, "response_base64")?;
            {
                let map = state.borrow_mut::<RuntimeStateMap>();
                if let Some(execution) = map.get_mut(&execution_id) {
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
            }
            tracing::debug!(%request_id, %call_index, host = %host, port = %port, "replay: returned recorded tcp exchange");
            return Ok(serde_json::json!({
                "bytes": bytes,
                "replay": true,
            }));
        }
        tracing::warn!(%request_id, %call_index, host = %host, port = %port, "replay: no recorded tcp checkpoint, making live exchange");
    }

    let request_json = serde_json::json!({
        "host": host,
        "port": port,
        "write_base64": base64::engine::general_purpose::STANDARD.encode(&write_bytes),
        "read_mode": read_mode,
        "read_bytes": read_bytes,
        "tls": tls,
        "server_name": server_name.clone(),
    });

    let started = std::time::Instant::now();
    let response_bytes = make_tcp_exchange(
        &host,
        port,
        &write_bytes,
        &read_mode,
        read_bytes,
        connect_timeout_ms,
        read_timeout_ms,
        tls,
        server_name.as_deref(),
        ca_cert_pem.as_deref(),
    )?;
    let duration_ms = started.elapsed().as_millis() as i32;

    let response_json = serde_json::json!({
        "host": host,
        "port": port,
        "response_base64": base64::engine::general_purpose::STANDARD.encode(&response_bytes),
        "bytes_read": response_bytes.len(),
        "replay": false,
        "tls": tls,
    });

    {
        let map = state.borrow_mut::<RuntimeStateMap>();
        if let Some(execution) = map.get_mut(&execution_id) {
            execution.checkpoints.push(FetchCheckpoint {
                call_index,
                boundary: "tcp".to_string(),
                url: format!("tcp://{}:{}", host, port),
                method: "exchange".to_string(),
                request: request_json,
                response: response_json,
                duration_ms,
            });
        }
    }

    tracing::debug!(%request_id, %call_index, host = %host, port = %port, "intercepted tcp exchange");

    Ok(serde_json::json!({
        "bytes": response_bytes,
        "replay": false,
    }))
}

#[op2]
#[serde]
fn op_flux_postgres_connect(
    state: &mut OpState,
    #[serde] request: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let PostgresConnectRequestPayload {
        execution_id,
        connection_string,
        tls,
        ca_cert_pem,
    } = serde_json::from_value(request)
        .map_err(|err| JsErrorBox::type_error(format!("invalid postgres connect request: {err}")))?;

    let target = parse_postgres_target(&connection_string)?;
    validate_outbound_host(&target.host, target.port, "postgres connect", "FLOWBASE_ALLOW_LOOPBACK_POSTGRES")?;

    let (request_id, mode, session_id) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "InternalError",
                format!("op_flux_postgres_connect: execution_id '{execution_id}' not found"),
            )
        })?;
        let session_id = format!("pg-session-{}", execution.next_postgres_session_id);
        execution.next_postgres_session_id = execution.next_postgres_session_id.saturating_add(1);
        (
            execution.context.request_id.clone(),
            execution.context.mode.clone(),
            session_id,
        )
    };

    if matches!(mode, ExecutionMode::Replay) {
        tracing::debug!(%request_id, session_id = %session_id, host = %target.host, port = %target.port, "replay: opened synthetic postgres session");
        return Ok(serde_json::json!({
            "sessionId": session_id,
            "replay": true,
        }));
    }

    let handle = PostgresSessionHandle::connect(
        &connection_string,
        &PostgresTlsOptions {
            enabled: tls,
            ca_cert_pem,
        },
        target.clone(),
    )?;

    {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "InternalError",
                format!("op_flux_postgres_connect: execution_id '{execution_id}' disappeared during connect"),
            )
        })?;
        execution.postgres_sessions.insert(session_id.clone(), handle);
    }

    tracing::debug!(%request_id, session_id = %session_id, host = %target.host, port = %target.port, "opened postgres session");

    Ok(serde_json::json!({
        "sessionId": session_id,
        "replay": false,
    }))
}

#[op2]
#[serde]
fn op_flux_postgres_simple_query(
    state: &mut OpState,
    #[serde] request: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let PostgresQueryRequestPayload {
        execution_id,
        connection_string,
        sql,
        params: _,
        tls,
        ca_cert_pem,
    } = serde_json::from_value(request)
        .map_err(|err| JsErrorBox::type_error(format!("invalid postgres query request: {err}")))?;

    let parsed_url = Url::parse(&connection_string)
        .map_err(|err| JsErrorBox::type_error(format!("invalid postgres connection string: {err}")))?;
    if !matches!(parsed_url.scheme(), "postgres" | "postgresql") {
        return Err(JsErrorBox::type_error("postgres connection string must use postgres:// or postgresql://"));
    }
    let host = parsed_url
        .host_str()
        .ok_or_else(|| JsErrorBox::type_error("postgres connection string missing host"))?;
    let port = parsed_url.port().unwrap_or(5432);
    validate_outbound_host(host, port, "postgres connect", "FLOWBASE_ALLOW_LOOPBACK_POSTGRES")?;

    let (request_id, call_index, mode, recorded_checkpoint) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "InternalError",
                format!("op_flux_postgres_simple_query: execution_id '{execution_id}' not found"),
            )
        })?;
        let idx = execution.call_index;
        execution.call_index = execution.call_index.saturating_add(1);
        let recorded = execution.recorded.remove(&idx);
        (
            execution.context.request_id.clone(),
            idx,
            execution.context.mode.clone(),
            recorded,
        )
    };

    if matches!(mode, ExecutionMode::Replay) {
        if let Some(checkpoint) = recorded_checkpoint {
            let response = checkpoint.response.clone();
            {
                let map = state.borrow_mut::<RuntimeStateMap>();
                if let Some(execution) = map.get_mut(&execution_id) {
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
            }
            tracing::debug!(%request_id, %call_index, host = %host, port = %port, "replay: returned recorded postgres query");
            return Ok(serde_json::json!({
                "rows": response.get("rows").cloned().unwrap_or_else(|| serde_json::json!([])),
                "command": response.get("command").cloned().unwrap_or(serde_json::Value::Null),
                "replay": true,
            }));
        }
        tracing::warn!(%request_id, %call_index, host = %host, port = %port, "replay: no recorded postgres checkpoint, making live query");
    }

    let started = std::time::Instant::now();
    let live_response = perform_postgres_simple_query(
        &connection_string,
        &sql,
        &PostgresTlsOptions {
            enabled: tls,
            ca_cert_pem,
        },
    )?;
    let duration_ms = started.elapsed().as_millis() as i32;

    let request_json = serde_json::json!({
        "url": format!("postgres://{}:{}/{}", host, port, parsed_url.path().trim_start_matches('/')),
        "host": host,
        "port": port,
        "sql": sql,
        "tls": tls,
    });
    let response_json = serde_json::json!({
        "rows": live_response.rows,
        "command": live_response.command,
        "row_count": live_response.row_count,
        "replay": false,
    });

    {
        let map = state.borrow_mut::<RuntimeStateMap>();
        if let Some(execution) = map.get_mut(&execution_id) {
            execution.checkpoints.push(FetchCheckpoint {
                call_index,
                boundary: "postgres".to_string(),
                url: format!("postgres://{}:{}/{}", host, port, parsed_url.path().trim_start_matches('/')),
                method: "simple_query".to_string(),
                request: request_json,
                response: response_json.clone(),
                duration_ms,
            });
        }
    }

    tracing::debug!(%request_id, %call_index, host = %host, port = %port, "intercepted postgres query");

    Ok(serde_json::json!({
        "rows": response_json.get("rows").cloned().unwrap_or_else(|| serde_json::json!([])),
        "command": response_json.get("command").cloned().unwrap_or(serde_json::Value::Null),
        "replay": false,
    }))
}

#[op2]
#[serde]
fn op_flux_postgres_session_query(
    state: &mut OpState,
    #[serde] request: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let PostgresSessionQueryRequestPayload {
        execution_id,
        session_id,
        sql,
        params,
    } = serde_json::from_value(request)
        .map_err(|err| JsErrorBox::type_error(format!("invalid postgres session query request: {err}")))?;

    let (request_id, call_index, mode, recorded_checkpoint) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "InternalError",
                format!("op_flux_postgres_session_query: execution_id '{execution_id}' not found"),
            )
        })?;
        let idx = execution.call_index;
        execution.call_index = execution.call_index.saturating_add(1);
        let recorded = execution.recorded.remove(&idx);
        (
            execution.context.request_id.clone(),
            idx,
            execution.context.mode.clone(),
            recorded,
        )
    };

    if matches!(mode, ExecutionMode::Replay) {
        if let Some(checkpoint) = recorded_checkpoint {
            let response = checkpoint.response.clone();
            {
                let map = state.borrow_mut::<RuntimeStateMap>();
                if let Some(execution) = map.get_mut(&execution_id) {
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
            }
            tracing::debug!(%request_id, %call_index, session_id = %session_id, "replay: returned recorded postgres session query");
            return Ok(serde_json::json!({
                "rows": response.get("rows").cloned().unwrap_or_else(|| serde_json::json!([])),
                "command": response.get("command").cloned().unwrap_or(serde_json::Value::Null),
                "replay": true,
            }));
        }
        tracing::warn!(%request_id, %call_index, session_id = %session_id, "replay: no recorded postgres session-query checkpoint, making live query");
    }

    let (target, tls_enabled, live_response, duration_ms) = {
        let started = std::time::Instant::now();
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "InternalError",
                format!("op_flux_postgres_session_query: execution_id '{execution_id}' disappeared before query"),
            )
        })?;
        let session = execution.postgres_sessions.get(&session_id).ok_or_else(|| {
            JsErrorBox::type_error(format!("postgres session '{session_id}' is not open"))
        })?;
        let response = session.query(&sql, &params)?;
        (
            session.target.clone(),
            session.tls,
            response,
            started.elapsed().as_millis() as i32,
        )
    };

    let request_json = serde_json::json!({
        "url": target.url,
        "host": target.host,
        "port": target.port,
        "sql": sql,
        "params": params,
        "tls": tls_enabled,
        "session": true,
    });
    let response_json = serde_json::json!({
        "rows": live_response.rows,
        "command": live_response.command,
        "row_count": live_response.row_count,
        "replay": false,
    });

    {
        let map = state.borrow_mut::<RuntimeStateMap>();
        if let Some(execution) = map.get_mut(&execution_id) {
            execution.checkpoints.push(FetchCheckpoint {
                call_index,
                boundary: "postgres".to_string(),
                url: target.url.clone(),
                method: "query".to_string(),
                request: request_json,
                response: response_json.clone(),
                duration_ms,
            });
        }
    }

    tracing::debug!(%request_id, %call_index, session_id = %session_id, host = %target.host, port = %target.port, "intercepted postgres session query");

    Ok(serde_json::json!({
        "rows": response_json.get("rows").cloned().unwrap_or_else(|| serde_json::json!([])),
        "command": response_json.get("command").cloned().unwrap_or(serde_json::Value::Null),
        "replay": false,
    }))
}

#[op2]
#[serde]
fn op_flux_postgres_query(
    state: &mut OpState,
    #[serde] request: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let PostgresQueryRequestPayload {
        execution_id,
        connection_string,
        sql,
        params,
        tls,
        ca_cert_pem,
    } = serde_json::from_value(request)
        .map_err(|err| JsErrorBox::type_error(format!("invalid postgres query request: {err}")))?;

    let parsed_url = Url::parse(&connection_string)
        .map_err(|err| JsErrorBox::type_error(format!("invalid postgres connection string: {err}")))?;
    if !matches!(parsed_url.scheme(), "postgres" | "postgresql") {
        return Err(JsErrorBox::type_error("postgres connection string must use postgres:// or postgresql://"));
    }
    let host = parsed_url
        .host_str()
        .ok_or_else(|| JsErrorBox::type_error("postgres connection string missing host"))?;
    let port = parsed_url.port().unwrap_or(5432);
    validate_outbound_host(host, port, "postgres connect", "FLOWBASE_ALLOW_LOOPBACK_POSTGRES")?;

    let (request_id, call_index, mode, recorded_checkpoint) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "InternalError",
                format!("op_flux_postgres_query: execution_id '{execution_id}' not found"),
            )
        })?;
        let idx = execution.call_index;
        execution.call_index = execution.call_index.saturating_add(1);
        let recorded = execution.recorded.remove(&idx);
        (
            execution.context.request_id.clone(),
            idx,
            execution.context.mode.clone(),
            recorded,
        )
    };

    if matches!(mode, ExecutionMode::Replay) {
        if let Some(checkpoint) = recorded_checkpoint {
            let response = checkpoint.response.clone();
            {
                let map = state.borrow_mut::<RuntimeStateMap>();
                if let Some(execution) = map.get_mut(&execution_id) {
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
            }
            tracing::debug!(%request_id, %call_index, host = %host, port = %port, "replay: returned recorded postgres prepared query");
            return Ok(serde_json::json!({
                "rows": response.get("rows").cloned().unwrap_or_else(|| serde_json::json!([])),
                "command": response.get("command").cloned().unwrap_or(serde_json::Value::Null),
                "replay": true,
            }));
        }
        tracing::warn!(%request_id, %call_index, host = %host, port = %port, "replay: no recorded postgres prepared-query checkpoint, making live query");
    }

    let started = std::time::Instant::now();
    let live_response = perform_postgres_query(
        &connection_string,
        &sql,
        &params,
        &PostgresTlsOptions {
            enabled: tls,
            ca_cert_pem,
        },
    )?;
    let duration_ms = started.elapsed().as_millis() as i32;

    let request_json = serde_json::json!({
        "url": format!("postgres://{}:{}/{}", host, port, parsed_url.path().trim_start_matches('/')),
        "host": host,
        "port": port,
        "sql": sql,
        "params": params,
        "tls": tls,
    });
    let response_json = serde_json::json!({
        "rows": live_response.rows,
        "command": live_response.command,
        "row_count": live_response.row_count,
        "replay": false,
    });

    {
        let map = state.borrow_mut::<RuntimeStateMap>();
        if let Some(execution) = map.get_mut(&execution_id) {
            execution.checkpoints.push(FetchCheckpoint {
                call_index,
                boundary: "postgres".to_string(),
                url: format!("postgres://{}:{}/{}", host, port, parsed_url.path().trim_start_matches('/')),
                method: "query".to_string(),
                request: request_json,
                response: response_json.clone(),
                duration_ms,
            });
        }
    }

    tracing::debug!(%request_id, %call_index, host = %host, port = %port, "intercepted postgres prepared query");

    Ok(serde_json::json!({
        "rows": response_json.get("rows").cloned().unwrap_or_else(|| serde_json::json!([])),
        "command": response_json.get("command").cloned().unwrap_or(serde_json::Value::Null),
        "replay": false,
    }))
}

#[op2]
#[serde]
fn op_flux_postgres_close_session(
    state: &mut OpState,
    #[serde] request: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let PostgresSessionCloseRequestPayload {
        execution_id,
        session_id,
    } = serde_json::from_value(request)
        .map_err(|err| JsErrorBox::type_error(format!("invalid postgres session close request: {err}")))?;

    let (request_id, mode) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "InternalError",
                format!("op_flux_postgres_close_session: execution_id '{execution_id}' not found"),
            )
        })?;
        (
            execution.context.request_id.clone(),
            execution.context.mode.clone(),
        )
    };

    if matches!(mode, ExecutionMode::Replay) {
        tracing::debug!(%request_id, session_id = %session_id, "replay: closed synthetic postgres session");
        return Ok(serde_json::json!({ "closed": true, "replay": true }));
    }

    let removed = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "InternalError",
                format!("op_flux_postgres_close_session: execution_id '{execution_id}' disappeared during close"),
            )
        })?;
        execution.postgres_sessions.remove(&session_id)
    };

    tracing::debug!(%request_id, session_id = %session_id, closed = removed.is_some(), "closed postgres session");

    Ok(serde_json::json!({ "closed": removed.is_some(), "replay": false }))
}

#[derive(Debug)]
struct PostgresSimpleQueryResponse {
    rows: Vec<serde_json::Value>,
    command: Option<String>,
    row_count: usize,
}

fn perform_postgres_simple_query(
    connection_string: &str,
    sql: &str,
    tls: &PostgresTlsOptions,
) -> Result<PostgresSimpleQueryResponse, JsErrorBox> {
    let connection_string = connection_string.to_string();
    let sql = sql.to_string();
    let tls = tls.clone();
    std::thread::spawn(move || {
        let mut client = connect_postgres_client(&connection_string, &tls)?;
        perform_postgres_simple_query_with_client(&mut client, &sql)
            .map_err(JsErrorBox::type_error)
    })
    .join()
    .map_err(|_| JsErrorBox::generic("postgres query thread panicked"))?
}

fn perform_postgres_query(
    connection_string: &str,
    sql: &str,
    params: &[serde_json::Value],
    tls: &PostgresTlsOptions,
) -> Result<PostgresSimpleQueryResponse, JsErrorBox> {
    let connection_string = connection_string.to_string();
    let sql = sql.to_string();
    let params = params.to_vec();
    let tls = tls.clone();
    std::thread::spawn(move || {
        let mut client = connect_postgres_client(&connection_string, &tls)?;
        perform_postgres_query_with_client(&mut client, &sql, &params)
            .map_err(JsErrorBox::type_error)
    })
    .join()
    .map_err(|_| JsErrorBox::generic("postgres query thread panicked"))?
}

fn perform_postgres_simple_query_with_client(
    client: &mut PostgresClient,
    sql: &str,
) -> std::result::Result<PostgresSimpleQueryResponse, String> {
    let messages = client
        .simple_query(sql)
        .map_err(|err| format!("postgres query failed: {err}"))?;

    let mut rows = Vec::new();
    let mut command = None;
    let command_name = sql
        .split_whitespace()
        .next()
        .map(|value| value.to_ascii_uppercase())
        .unwrap_or_else(|| "QUERY".to_string());
    for message in messages {
        match message {
            SimpleQueryMessage::Row(row) => {
                let mut object = serde_json::Map::new();
                for idx in 0..row.len() {
                    let key = row.columns()[idx].name().to_string();
                    let value = row
                        .get(idx)
                        .map(|value| serde_json::Value::String(value.to_string()))
                        .unwrap_or(serde_json::Value::Null);
                    object.insert(key, value);
                }
                rows.push(serde_json::Value::Object(object));
            }
            SimpleQueryMessage::CommandComplete(count) => {
                command = Some(format!("{} {}", command_name, count));
            }
            _ => {}
        }
    }

    Ok(PostgresSimpleQueryResponse {
        row_count: rows.len(),
        rows,
        command,
    })
}

fn perform_postgres_query_with_client(
    client: &mut PostgresClient,
    sql: &str,
    params: &[serde_json::Value],
) -> std::result::Result<PostgresSimpleQueryResponse, String> {
    let mut boxed_params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
    for param in params.iter().cloned() {
        boxed_params.push(box_postgres_param(param).map_err(|err| err.to_string())?);
    }
    let refs: Vec<&(dyn ToSql + Sync)> = boxed_params
        .iter()
        .map(|value| value.as_ref() as &(dyn ToSql + Sync))
        .collect();

    let query_rows = client
        .query(sql, &refs)
        .map_err(|err| format!("postgres query failed: {err}"))?;

    let mut rows = Vec::new();
    for row in query_rows {
        let mut object = serde_json::Map::new();
        for column in row.columns() {
            let key = column.name().to_string();
            let value = decode_postgres_row_value(&row, column);
            object.insert(key, value);
        }
        rows.push(serde_json::Value::Object(object));
    }

    Ok(PostgresSimpleQueryResponse {
        row_count: rows.len(),
        rows,
        command: Some("QUERY".to_string()),
    })
}

fn parse_postgres_target(connection_string: &str) -> Result<PostgresConnectionTarget, JsErrorBox> {
    let parsed_url = Url::parse(connection_string)
        .map_err(|err| JsErrorBox::type_error(format!("invalid postgres connection string: {err}")))?;
    if !matches!(parsed_url.scheme(), "postgres" | "postgresql") {
        return Err(JsErrorBox::type_error("postgres connection string must use postgres:// or postgresql://"));
    }
    let host = parsed_url
        .host_str()
        .ok_or_else(|| JsErrorBox::type_error("postgres connection string missing host"))?
        .to_string();
    let port = parsed_url.port().unwrap_or(5432);

    Ok(PostgresConnectionTarget {
        url: format!("postgres://{}:{}/{}", host, port, parsed_url.path().trim_start_matches('/')),
        host,
        port,
    })
}

fn connect_postgres_client(
    connection_string: &str,
    tls: &PostgresTlsOptions,
) -> Result<PostgresClient, JsErrorBox> {
    let mut config: PostgresConfig = connection_string
        .parse()
        .map_err(|err| JsErrorBox::type_error(format!("invalid postgres connection string: {err}")))?;

    if tls.enabled {
        config.ssl_mode(PostgresSslMode::Require);
        let tls_config = build_tls_client_config(tls.ca_cert_pem.as_deref())?;
        let connector = PostgresMakeTlsConnector::new(TlsConnector::from(tls_config));
        config
            .connect(connector)
            .map_err(|err| JsErrorBox::type_error(format!("postgres connect failed: {err}")))
    } else {
        config.ssl_mode(PostgresSslMode::Disable);
        config
            .connect(NoTls)
            .map_err(|err| JsErrorBox::type_error(format!("postgres connect failed: {err}")))
    }
}

fn box_postgres_param(param: serde_json::Value) -> Result<Box<dyn ToSql + Sync>, JsErrorBox> {
    match param {
        serde_json::Value::Null => Ok(Box::new(Option::<String>::None)),
        serde_json::Value::String(value) => Ok(Box::new(value)),
        serde_json::Value::Bool(value) => Ok(Box::new(value)),
        serde_json::Value::Number(value) => {
            if let Some(signed) = value.as_i64() {
                Ok(Box::new(signed))
            } else if let Some(unsigned) = value.as_u64() {
                if let Ok(signed) = i64::try_from(unsigned) {
                    Ok(Box::new(signed))
                } else {
                    Ok(Box::new(unsigned.to_string()))
                }
            } else if let Some(float) = value.as_f64() {
                Ok(Box::new(float))
            } else {
                Err(JsErrorBox::type_error(format!(
                    "unsupported postgres numeric parameter: {}",
                    value
                )))
            }
        }
        other => Err(JsErrorBox::type_error(format!(
            "unsupported postgres parameter type: {}",
            other
        ))),
    }
}

fn decode_postgres_row_value(row: &postgres::Row, column: &postgres::Column) -> serde_json::Value {
    let name = column.name();
    let ty = column.type_();
    let string_value = || {
        row.try_get::<_, Option<String>>(name)
            .ok()
            .flatten()
    };

    match *ty {
        postgres::types::Type::BOOL => row
            .try_get::<_, Option<bool>>(name)
            .ok()
            .flatten()
            .map(serde_json::Value::Bool)
            .or_else(|| {
                string_value().and_then(|value| match value.as_str() {
                    "t" | "true" => Some(serde_json::Value::Bool(true)),
                    "f" | "false" => Some(serde_json::Value::Bool(false)),
                    _ => None,
                })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INT2 => row
            .try_get::<_, Option<i16>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::json!(value))
            .or_else(|| string_value().and_then(|value| value.parse::<i16>().ok().map(|parsed| serde_json::json!(parsed))))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INT4 => row
            .try_get::<_, Option<i32>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::json!(value))
            .or_else(|| string_value().and_then(|value| value.parse::<i32>().ok().map(|parsed| serde_json::json!(parsed))))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INT8 => row
            .try_get::<_, Option<i64>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::json!(value))
            .or_else(|| string_value().and_then(|value| value.parse::<i64>().ok().map(|parsed| serde_json::json!(parsed))))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::FLOAT4 => row
            .try_get::<_, Option<f32>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::json!(value))
            .or_else(|| string_value().and_then(|value| value.parse::<f32>().ok().map(|parsed| serde_json::json!(parsed))))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::FLOAT8 => row
            .try_get::<_, Option<f64>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::json!(value))
            .or_else(|| string_value().and_then(|value| value.parse::<f64>().ok().map(|parsed| serde_json::json!(parsed))))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::NUMERIC => string_value()
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::JSON | postgres::types::Type::JSONB => row
            .try_get::<_, Option<serde_json::Value>>(name)
            .ok()
            .flatten()
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::BOOL_ARRAY => row
            .try_get::<_, Option<Vec<bool>>>(name)
            .ok()
            .flatten()
            .map(|values| serde_json::Value::Array(values.into_iter().map(serde_json::Value::Bool).collect()))
            .or_else(|| string_value().and_then(|value| parse_postgres_text_array(&value, parse_postgres_bool_array_element)))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INT2_ARRAY => row
            .try_get::<_, Option<Vec<i16>>>(name)
            .ok()
            .flatten()
            .map(|values| serde_json::Value::Array(values.into_iter().map(|value| serde_json::json!(value)).collect()))
            .or_else(|| string_value().and_then(|value| parse_postgres_text_array(&value, |item| item.parse::<i16>().ok().map(|parsed| serde_json::json!(parsed)))))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INT4_ARRAY => row
            .try_get::<_, Option<Vec<i32>>>(name)
            .ok()
            .flatten()
            .map(|values| serde_json::Value::Array(values.into_iter().map(|value| serde_json::json!(value)).collect()))
            .or_else(|| string_value().and_then(|value| parse_postgres_text_array(&value, |item| item.parse::<i32>().ok().map(|parsed| serde_json::json!(parsed)))))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INT8_ARRAY => row
            .try_get::<_, Option<Vec<i64>>>(name)
            .ok()
            .flatten()
            .map(|values| serde_json::Value::Array(values.into_iter().map(|value| serde_json::json!(value)).collect()))
            .or_else(|| string_value().and_then(|value| parse_postgres_text_array(&value, |item| item.parse::<i64>().ok().map(|parsed| serde_json::json!(parsed)))))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::FLOAT4_ARRAY => row
            .try_get::<_, Option<Vec<f32>>>(name)
            .ok()
            .flatten()
            .map(|values| serde_json::Value::Array(values.into_iter().map(|value| serde_json::json!(value)).collect()))
            .or_else(|| string_value().and_then(|value| parse_postgres_text_array(&value, |item| item.parse::<f32>().ok().map(|parsed| serde_json::json!(parsed)))))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::FLOAT8_ARRAY => row
            .try_get::<_, Option<Vec<f64>>>(name)
            .ok()
            .flatten()
            .map(|values| serde_json::Value::Array(values.into_iter().map(|value| serde_json::json!(value)).collect()))
            .or_else(|| string_value().and_then(|value| parse_postgres_text_array(&value, |item| item.parse::<f64>().ok().map(|parsed| serde_json::json!(parsed)))))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::TEXT_ARRAY | postgres::types::Type::VARCHAR_ARRAY => row
            .try_get::<_, Option<Vec<String>>>(name)
            .ok()
            .flatten()
            .map(|values| serde_json::Value::Array(values.into_iter().map(serde_json::Value::String).collect()))
            .or_else(|| string_value().and_then(|value| parse_postgres_text_array(&value, |item| Some(serde_json::Value::String(item.to_string())))))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::NUMERIC_ARRAY => string_value()
            .and_then(|value| parse_postgres_text_array(&value, |item| Some(serde_json::Value::String(item.to_string()))))
            .unwrap_or(serde_json::Value::Null),
        _ => string_value()
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null),
    }
}

fn parse_postgres_bool_array_element(item: &str) -> Option<serde_json::Value> {
    match item {
        "t" | "true" => Some(serde_json::Value::Bool(true)),
        "f" | "false" => Some(serde_json::Value::Bool(false)),
        _ => None,
    }
}

fn parse_postgres_text_array<F>(
    raw: &str,
    parse_item: F,
) -> Option<serde_json::Value>
where
    F: Fn(&str) -> Option<serde_json::Value>,
{
    if !raw.starts_with('{') || !raw.ends_with('}') {
        return None;
    }

    let inner = &raw[1..raw.len().saturating_sub(1)];
    if inner.is_empty() {
        return Some(serde_json::Value::Array(Vec::new()));
    }

    let mut values = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    let mut quoted_current = false;

    let push_current = |values: &mut Vec<serde_json::Value>, current: &mut String, quoted_current: &mut bool| {
        let value = if !*quoted_current && current == "NULL" {
            serde_json::Value::Null
        } else {
            parse_item(current).unwrap_or(serde_json::Value::Null)
        };
        values.push(value);
        current.clear();
        *quoted_current = false;
    };

    for ch in inner.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' if in_quotes => escaped = true,
            '"' => {
                in_quotes = !in_quotes;
                if in_quotes && current.is_empty() {
                    quoted_current = true;
                }
            }
            ',' if !in_quotes => push_current(&mut values, &mut current, &mut quoted_current),
            other => current.push(other),
        }
    }

    if in_quotes || escaped {
        return None;
    }

    push_current(&mut values, &mut current, &mut quoted_current);
    Some(serde_json::Value::Array(values))
}

#[cfg(test)]
mod tests {
    use super::{parse_postgres_bool_array_element, parse_postgres_text_array};

    #[test]
    fn parses_text_arrays() {
        let parsed = parse_postgres_text_array("{alpha,beta}", |item| {
            Some(serde_json::Value::String(item.to_string()))
        });

        assert_eq!(
            parsed,
            Some(serde_json::json!(["alpha", "beta"]))
        );
    }

    #[test]
    fn parses_quoted_text_and_null_arrays() {
        let parsed = parse_postgres_text_array("{\"hello,world\",NULL,plain}", |item| {
            Some(serde_json::Value::String(item.to_string()))
        });

        assert_eq!(
            parsed,
            Some(serde_json::json!(["hello,world", null, "plain"]))
        );
    }

    #[test]
    fn parses_bool_arrays() {
        let parsed = parse_postgres_text_array("{t,f,true,false}", parse_postgres_bool_array_element);

        assert_eq!(
            parsed,
            Some(serde_json::json!([true, false, true, false]))
        );
    }
}

fn decode_checkpoint_bytes(
    value: &serde_json::Value,
    field: &str,
) -> Result<Vec<u8>, JsErrorBox> {
    let encoded = value
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| JsErrorBox::type_error(format!("recorded tcp checkpoint missing {field}")))?;
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|err| JsErrorBox::type_error(format!("invalid recorded tcp payload: {err}")))
}

fn make_tcp_exchange(
    host: &str,
    port: u16,
    write_bytes: &[u8],
    read_mode: &str,
    read_bytes: Option<usize>,
    connect_timeout_ms: Option<u64>,
    read_timeout_ms: Option<u64>,
    tls: bool,
    server_name: Option<&str>,
    ca_cert_pem: Option<&str>,
) -> Result<Vec<u8>, JsErrorBox> {
    let connect_timeout = Duration::from_millis(connect_timeout_ms.unwrap_or(DEFAULT_CONNECT_TIMEOUT_MS));
    let read_timeout = Duration::from_millis(read_timeout_ms.unwrap_or(DEFAULT_READ_TIMEOUT_MS));

    let mut addrs = (host, port)
        .to_socket_addrs()
        .map_err(|err| JsErrorBox::type_error(format!("tcp connect failed: {err}")))?;
    let addr = addrs
        .next()
        .ok_or_else(|| JsErrorBox::type_error("tcp connect failed: no resolved address"))?;

    let mut stream = TcpStream::connect_timeout(&addr, connect_timeout)
        .map_err(|err| JsErrorBox::type_error(format!("tcp connect failed: {err}")))?;
    stream
        .set_read_timeout(Some(read_timeout))
        .map_err(|err| JsErrorBox::type_error(format!("tcp read timeout setup failed: {err}")))?;
    stream
        .set_write_timeout(Some(connect_timeout))
        .map_err(|err| JsErrorBox::type_error(format!("tcp write timeout setup failed: {err}")))?;

    if tls {
        let tls_server_name = server_name.unwrap_or(host);
        let client_config = build_tls_client_config(ca_cert_pem)?;
        let server_name = ServerName::try_from(tls_server_name.to_string())
            .map_err(|err| JsErrorBox::type_error(format!("invalid TLS server name: {err}")))?;
        let connection = ClientConnection::new(client_config, server_name)
            .map_err(|err| JsErrorBox::type_error(format!("tls connect failed: {err}")))?;
        let mut tls_stream = StreamOwned::new(connection, stream);
        perform_buffered_exchange(&mut tls_stream, write_bytes, read_mode, read_bytes)
    } else {
        perform_plain_buffered_exchange(&mut stream, write_bytes, read_mode, read_bytes)
    }
}

fn build_tls_client_config(ca_cert_pem: Option<&str>) -> Result<Arc<ClientConfig>, JsErrorBox> {
    ensure_rustls_provider();

    let mut roots = RootCertStore::empty();

    if let Some(pem) = ca_cert_pem {
        let mut reader = std::io::BufReader::new(pem.as_bytes());
        let certs = rustls_pemfile::certs(&mut reader)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|err| JsErrorBox::type_error(format!("invalid CA certificate PEM: {err}")))?;

        if certs.is_empty() {
            return Err(JsErrorBox::type_error("invalid CA certificate PEM: no certificates found"));
        }

        for cert in certs {
            roots
                .add(cert)
                .map_err(|err| JsErrorBox::type_error(format!("invalid CA certificate PEM: {err}")))?;
        }
    } else {
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    }

    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    Ok(Arc::new(config))
}

fn ensure_rustls_provider() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

fn perform_buffered_exchange<T: Read + Write>(
    stream: &mut T,
    write_bytes: &[u8],
    read_mode: &str,
    read_bytes: Option<usize>,
) -> Result<Vec<u8>, JsErrorBox> {
    if !write_bytes.is_empty() {
        stream
            .write_all(write_bytes)
            .map_err(|err| JsErrorBox::type_error(format!("tcp write failed: {err}")))?;
    }
    stream
        .flush()
        .map_err(|err| JsErrorBox::type_error(format!("tcp flush failed: {err}")))?;

    match read_mode {
        "until_close" => {
            let mut bytes = Vec::new();
            stream
                .take((MAX_RESPONSE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|err| JsErrorBox::type_error(format!("tcp read failed: {err}")))?;

            if bytes.len() > MAX_RESPONSE_BYTES {
                return Err(JsErrorBox::type_error(format!(
                    "tcp response too large: {} bytes exceeds {MAX_RESPONSE_BYTES} byte limit",
                    bytes.len()
                )));
            }

            Ok(bytes)
        }
        "fixed" => {
            let expected = read_bytes.ok_or_else(|| {
                JsErrorBox::type_error("tcp fixed read mode requires readBytes")
            })?;
            if expected > MAX_RESPONSE_BYTES {
                return Err(JsErrorBox::type_error(format!(
                    "tcp response too large: {expected} bytes exceeds {MAX_RESPONSE_BYTES} byte limit"
                )));
            }
            let mut bytes = vec![0; expected];
            stream
                .read_exact(&mut bytes)
                .map_err(|err| JsErrorBox::type_error(format!("tcp read failed: {err}")))?;
            Ok(bytes)
        }
        other => Err(JsErrorBox::type_error(format!(
            "unsupported tcp read mode: {other}"
        ))),
    }
}

fn perform_plain_buffered_exchange(
    stream: &mut TcpStream,
    write_bytes: &[u8],
    read_mode: &str,
    read_bytes: Option<usize>,
) -> Result<Vec<u8>, JsErrorBox> {
    if !write_bytes.is_empty() {
        stream
            .write_all(write_bytes)
            .map_err(|err| JsErrorBox::type_error(format!("tcp write failed: {err}")))?;
    }
    stream
        .flush()
        .map_err(|err| JsErrorBox::type_error(format!("tcp flush failed: {err}")))?;
    stream
        .shutdown(Shutdown::Write)
        .map_err(|err| JsErrorBox::type_error(format!("tcp shutdown failed: {err}")))?;

    match read_mode {
        "until_close" => {
            let mut bytes = Vec::new();
            stream
                .take((MAX_RESPONSE_BYTES + 1) as u64)
                .read_to_end(&mut bytes)
                .map_err(|err| JsErrorBox::type_error(format!("tcp read failed: {err}")))?;

            if bytes.len() > MAX_RESPONSE_BYTES {
                return Err(JsErrorBox::type_error(format!(
                    "tcp response too large: {} bytes exceeds {MAX_RESPONSE_BYTES} byte limit",
                    bytes.len()
                )));
            }

            Ok(bytes)
        }
        "fixed" => {
            let expected = read_bytes.ok_or_else(|| {
                JsErrorBox::type_error("tcp fixed read mode requires readBytes")
            })?;
            if expected > MAX_RESPONSE_BYTES {
                return Err(JsErrorBox::type_error(format!(
                    "tcp response too large: {expected} bytes exceeds {MAX_RESPONSE_BYTES} byte limit"
                )));
            }
            let mut bytes = vec![0; expected];
            stream
                .read_exact(&mut bytes)
                .map_err(|err| JsErrorBox::type_error(format!("tcp read failed: {err}")))?;
            Ok(bytes)
        }
        other => Err(JsErrorBox::type_error(format!(
            "unsupported tcp read mode: {other}"
        ))),
    }
}

/// Returns current time as milliseconds since Unix epoch.
/// In Replay mode returns the timestamp recorded during the original Live execution,
/// making `Date.now()` deterministic across replays.
#[op2(fast)]
fn op_flux_now(state: &mut OpState, #[string] execution_id: String) -> f64 {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let map = state.borrow_mut::<RuntimeStateMap>();
    let exec = match map.get_mut(&execution_id) {
        Some(e) => e,
        None => return now_ms as f64,
    };

    match exec.context.mode {
        ExecutionMode::Replay => exec.recorded_now_ms.unwrap_or(now_ms) as f64,
        ExecutionMode::Live => {
            if exec.recorded_now_ms.is_none() {
                exec.recorded_now_ms = Some(now_ms);
            }
            now_ms as f64
        }
    }
}

#[op2]
#[serde]
fn op_flux_parse_url(
    #[string] input: String,
    #[string] base: String,
) -> std::result::Result<serde_json::Value, JsErrorBox> {
    let parsed = if base.is_empty() {
        url::Url::parse(&input)
    } else {
        let base_url = url::Url::parse(&base)
            .map_err(|err| JsErrorBox::type_error(format!("invalid base URL: {err}")))?;
        base_url.join(&input)
    }
    .map_err(|err| JsErrorBox::type_error(format!("invalid URL: {err}")))?;

    Ok(serde_json::json!({
        "href": parsed.as_str(),
        "origin": parsed.origin().ascii_serialization(),
        "protocol": format!("{}:", parsed.scheme()),
        "username": parsed.username(),
        "password": parsed.password(),
        "host": parsed.host_str().map(|host| {
            parsed.port().map(|port| format!("{host}:{port}")).unwrap_or_else(|| host.to_string())
        }).unwrap_or_default(),
        "hostname": parsed.host_str().unwrap_or_default(),
        "port": parsed.port().map(|port| port.to_string()).unwrap_or_default(),
        "pathname": parsed.path(),
        "search": parsed.query().map(|query| format!("?{query}")).unwrap_or_default(),
        "hash": parsed.fragment().map(|fragment| format!("#{fragment}")).unwrap_or_default(),
    }))
}

/// Captures `console.log/warn/error` output and links it to the current execution.
#[op2(fast)]
fn op_console(
    state: &mut OpState,
    #[string] execution_id: String,
    #[string] msg: String,
    is_err: bool,
) {
    let map = state.borrow_mut::<RuntimeStateMap>();
    if let Some(exec) = map.get_mut(&execution_id) {
        exec.logs.push(LogEntry {
            level: if is_err { "error".to_string() } else { "log".to_string() },
            message: msg.clone(),
        });
    }
    if is_err {
        eprintln!("{msg}");
    } else {
        println!("{msg}");
    }
}

/// Returns the effective timer delay to use.
/// Records timer boundaries so traces capture scheduling decisions.
#[op2(fast)]
fn op_timer_delay(state: &mut OpState, #[string] execution_id: String, delay_ms: f64) -> f64 {
    let map = state.borrow_mut::<RuntimeStateMap>();
    match map.get_mut(&execution_id) {
        Some(exec) => {
            let call_index = exec.call_index;
            exec.call_index = exec.call_index.saturating_add(1);

            match exec.context.mode {
                ExecutionMode::Replay => {
                    let recorded = exec.recorded.remove(&call_index);
                    let effective_delay_ms = recorded
                        .as_ref()
                        .and_then(|checkpoint| checkpoint.response.get("effective_delay_ms"))
                        .and_then(|value| value.as_f64())
                        .unwrap_or_else(|| if delay_ms.is_sign_negative() { 0.0 } else { delay_ms });

                    if let Some(checkpoint) = recorded {
                        exec.checkpoints.push(checkpoint);
                    } else {
                        exec.checkpoints.push(FetchCheckpoint {
                            call_index,
                            boundary: "timer".to_string(),
                            url: String::new(),
                            method: "delay".to_string(),
                            request: serde_json::json!({
                                "requested_delay_ms": delay_ms,
                            }),
                            response: serde_json::json!({
                                "effective_delay_ms": effective_delay_ms,
                                "replay": true,
                            }),
                            duration_ms: 0,
                        });
                    }

                    effective_delay_ms
                }
                ExecutionMode::Live => {
                    let effective_delay_ms = if delay_ms.is_sign_negative() { 0.0 } else { delay_ms };
                    exec.checkpoints.push(FetchCheckpoint {
                        call_index,
                        boundary: "timer".to_string(),
                        url: String::new(),
                        method: "delay".to_string(),
                        request: serde_json::json!({
                            "requested_delay_ms": delay_ms,
                        }),
                        response: serde_json::json!({
                            "effective_delay_ms": effective_delay_ms,
                            "replay": false,
                        }),
                        duration_ms: 0,
                    });
                    effective_delay_ms
                }
            }
        }
        None => delay_ms,
    }
}

/// In Live mode: generate a value via `rand`, record it for later storage.
/// In Replay mode: return the next recorded value in sequence (fallback: 0.5).
#[op2(fast)]
fn op_random(state: &mut OpState, #[string] execution_id: String) -> f64 {
    let map = state.borrow_mut::<RuntimeStateMap>();
    let exec = match map.get_mut(&execution_id) {
        Some(e) => e,
        None => return rand::thread_rng().r#gen(),
    };
    match exec.context.mode {
        ExecutionMode::Live => {
            let v: f64 = rand::thread_rng().r#gen();
            exec.recorded_random.push(v);
            v
        }
        ExecutionMode::Replay => {
            let idx = exec.random_index;
            exec.random_index += 1;
            exec.recorded_random.get(idx).copied().unwrap_or(0.5)
        }
    }
}

/// In Live mode: generate a UUID v4 and record it.
/// In Replay mode: return the recorded UUID in sequence.
#[op2]
#[string]
fn op_random_uuid(state: &mut OpState, #[string] execution_id: String) -> String {
    let map = state.borrow_mut::<RuntimeStateMap>();
    let exec = match map.get_mut(&execution_id) {
        Some(e) => e,
        None => return Uuid::new_v4().to_string(),
    };
    match exec.context.mode {
        ExecutionMode::Live => {
            let id = Uuid::new_v4().to_string();
            exec.recorded_uuids.push(id.clone());
            id
        }
        ExecutionMode::Replay => {
            let idx = exec.uuid_index;
            exec.uuid_index += 1;
            exec.recorded_uuids
                .get(idx)
                .cloned()
                .unwrap_or_else(|| Uuid::new_v4().to_string())
        }
    }
}

/// Intercepts `Deno.serve()` — marks the isolate as a long-running HTTP server.
#[op2(fast)]
fn op_net_listen(state: &mut OpState, #[string] execution_id: String, #[smi] _port: u32) {
    let map = state.borrow_mut::<RuntimeStateMap>();
    if let Some(exec) = map.get_mut(&execution_id) {
        exec.is_server_mode = true;
    }
}

/// Called by the `__flux_dispatch_request` JS shim after the handler produces
/// an HTTP response.  Stores the finalized response keyed by req_id.
#[op2(fast)]
fn op_net_respond(
    state: &mut OpState,
    #[string] execution_id: String,
    #[string] req_id: String,
    #[smi] status: u32,
    #[string] headers_json: String,
    #[string] body: String,
) {
    let headers: Vec<(String, String)> = serde_json::from_str::<Vec<Vec<String>>>(&headers_json)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|pair| {
            let mut it = pair.into_iter();
            let k = it.next()?;
            let v = it.next()?;
            Some((k, v))
        })
        .collect();

    let map = state.borrow_mut::<RuntimeStateMap>();
    if let Some(exec) = map.get_mut(&execution_id) {
        exec.pending_responses
            .insert(req_id, NetResponse { status: status as u16, headers, body });
    }
}

fn make_http_request(
    url: &str,
    method: &str,
    body: Option<String>,
    headers: Option<HashMap<String, String>>,
) -> Result<serde_json::Value, JsErrorBox> {
    let agent = ureq::builder().redirects(0).build();
    let request_headers = headers.unwrap_or_default();
    let mut current_url = url.to_string();
    let mut current_method = method.to_string();
    let mut current_body = body;
    let response = {
        let mut final_response = None;
        for redirect_count in 0..=MAX_REDIRECTS {
        validate_fetch_url(&current_url)?;

        let mut request = agent.request(&current_method, &current_url);
        for (key, value) in &request_headers {
            request = request.set(key, value);
        }

        let response = match current_body.as_deref() {
            Some(body) => request.send_string(body),
            None => request.call(),
        }
        .or_any_status()
        .map_err(|err| JsErrorBox::type_error(format!("fetch failed: {err}")))?;

        match response.status() {
            301 | 302 | 303 | 307 | 308 => {
                if redirect_count == MAX_REDIRECTS {
                    return Err(JsErrorBox::type_error("too many redirects"));
                }

                let location = response.header("location").ok_or_else(|| {
                    JsErrorBox::type_error("redirect response missing Location header")
                })?;

                let next_url = url::Url::parse(&current_url)
                    .and_then(|base| base.join(location))
                    .map_err(|err| JsErrorBox::type_error(format!("invalid redirect URL: {err}")))?
                    .to_string();

                if current_url == next_url {
                    return Err(JsErrorBox::type_error("redirect loop detected"));
                }

                match response.status() {
                    301 | 302 if current_method.eq_ignore_ascii_case("POST") => {
                        current_method = "GET".to_string();
                        current_body = None;
                    }
                    303 if !current_method.eq_ignore_ascii_case("HEAD") => {
                        current_method = "GET".to_string();
                        current_body = None;
                    }
                    _ => {}
                }

                current_url = next_url;
                continue;
            }
            _ => {
                final_response = Some(response);
                break;
            }
        }
        }
        final_response.ok_or_else(|| JsErrorBox::type_error("redirect resolution failed"))?
    };

    // Reject responses that advertise a body larger than our limit.
    if let Some(len) = response
        .header("content-length")
        .and_then(|value: &str| value.parse::<usize>().ok())
    {
        if len > MAX_RESPONSE_BYTES {
            return Err(JsErrorBox::type_error(
                format!("response too large: {len} bytes exceeds {MAX_RESPONSE_BYTES} byte limit"),
            ));
        }
    }

    let status = response.status();
    let response_headers: BTreeMap<String, String> = response
        .headers_names()
        .into_iter()
        .filter_map(|name: String| {
            response
                .header(&name)
                .map(|value: &str| (name.to_ascii_lowercase(), value.to_string()))
        })
        .collect();

    // Stream the body with a size cap to protect against missing/lying Content-Length.
    let reader = response.into_reader();
    let mut bytes = Vec::new();
    reader
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|err: std::io::Error| JsErrorBox::type_error(err.to_string()))?;

    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(JsErrorBox::type_error(
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

type SourceMapStore = Rc<RefCell<HashMap<String, Vec<u8>>>>;

struct TypescriptModuleLoader {
    source_maps: SourceMapStore,
}

struct ArtifactModuleLoader {
    source_maps: SourceMapStore,
    modules: HashMap<String, ArtifactModule>,
}

fn should_transpile_media_type(media_type: MediaType) -> bool {
    matches!(
        media_type,
        MediaType::Jsx
            | MediaType::TypeScript
            | MediaType::Mts
            | MediaType::Cts
            | MediaType::Dts
            | MediaType::Dmts
            | MediaType::Dcts
            | MediaType::Tsx
    )
}

fn transpile_module_source(
    module_specifier: &ModuleSpecifier,
    media_type: MediaType,
    source: String,
    source_maps: Option<&SourceMapStore>,
) -> std::result::Result<String, JsErrorBox> {
    if !should_transpile_media_type(media_type) {
        return Ok(source);
    }

    let result = deno_ast::parse_module(ParseParams {
        specifier: module_specifier.clone(),
        text: source.into(),
        media_type,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    })
    .map_err(JsErrorBox::from_err)?
    .transpile(
        &TranspileOptions::default(),
        &TranspileModuleOptions::default(),
        &EmitOptions {
            source_map: SourceMapOption::Separate,
            inline_sources: true,
            ..Default::default()
        },
    )
    .map_err(JsErrorBox::from_err)?
    .into_source();

    if let (Some(source_maps), Some(source_map)) = (source_maps, result.source_map) {
        source_maps
            .borrow_mut()
            .insert(module_specifier.to_string(), source_map.into_bytes());
    }

    Ok(result.text)
}

fn artifact_media_type(media_type: ArtifactMediaType) -> MediaType {
    match media_type {
        ArtifactMediaType::JavaScript => MediaType::JavaScript,
        ArtifactMediaType::Mjs => MediaType::Mjs,
        ArtifactMediaType::Jsx => MediaType::Jsx,
        ArtifactMediaType::TypeScript => MediaType::TypeScript,
        ArtifactMediaType::Tsx => MediaType::Tsx,
        ArtifactMediaType::Json => MediaType::Json,
    }
}

impl ModuleLoader for TypescriptModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> std::result::Result<ModuleSpecifier, deno_core::error::ModuleLoaderError> {
        resolve_import(specifier, referrer).map_err(JsErrorBox::from_err)
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let source_maps = self.source_maps.clone();
        let module_specifier = module_specifier.clone();

        fn load_module(
            source_maps: SourceMapStore,
            module_specifier: &ModuleSpecifier,
            options: &ModuleLoadOptions,
        ) -> std::result::Result<ModuleSource, deno_core::error::ModuleLoaderError> {
            let path = module_specifier
                .to_file_path()
                .map_err(|_| JsErrorBox::generic("Only file:// URLs are supported."))?;

            let media_type = MediaType::from_path(&path);
            let module_type = match media_type {
                MediaType::JavaScript | MediaType::Mjs | MediaType::Cjs => {
                    ModuleType::JavaScript
                }
                MediaType::TypeScript
                | MediaType::Mts
                | MediaType::Cts
                | MediaType::Dts
                | MediaType::Dmts
                | MediaType::Dcts
                | MediaType::Tsx
                | MediaType::Jsx => ModuleType::JavaScript,
                MediaType::Json => ModuleType::Json,
                _ => {
                    return Err(JsErrorBox::generic(format!(
                        "unsupported module extension: {}",
                        path.display()
                    )));
                }
            };

            if module_type == ModuleType::Json
                && options.requested_module_type != deno_core::RequestedModuleType::Json
            {
                return Err(JsErrorBox::generic(
                    "attempted to load JSON module without `with { type: \"json\" }`",
                ));
            }

            let source = std::fs::read_to_string(&path)
                .map_err(JsErrorBox::from_err)?;
            let source = transpile_module_source(
                module_specifier,
                media_type,
                source,
                Some(&source_maps),
            )?;

            Ok(ModuleSource::new(
                module_type,
                ModuleSourceCode::String(source.into()),
                module_specifier,
                None,
            ))
        }

        ModuleLoadResponse::Sync(load_module(source_maps, &module_specifier, &options))
    }

    fn get_source_map(&self, specifier: &str) -> Option<Cow<'_, [u8]>> {
        self.source_maps
            .borrow()
            .get(specifier)
            .map(|value| value.clone().into())
    }
}

impl ModuleLoader for ArtifactModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> std::result::Result<ModuleSpecifier, deno_core::error::ModuleLoaderError> {
        if specifier.starts_with("npm:") {
            let resolved = Url::parse(specifier).map_err(JsErrorBox::from_err)?;
            if self.modules.contains_key(resolved.as_str()) {
                return Ok(resolved);
            }
        }

        let resolution_base = self
            .modules
            .get(referrer)
            .map(|module| module.base_specifier.as_str())
            .unwrap_or(referrer);
        let resolved = resolve_import(specifier, resolution_base).map_err(JsErrorBox::from_err)?;

        if self.modules.contains_key(resolved.as_str()) {
            Ok(resolved)
        } else {
            Err(JsErrorBox::generic(format!(
                "dynamic resolution is disabled for built artifacts: {}",
                resolved
            ))
            .into())
        }
    }

    fn load(
        &self,
        module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<&ModuleLoadReferrer>,
        options: ModuleLoadOptions,
    ) -> ModuleLoadResponse {
        let source_maps = self.source_maps.clone();
        let module_specifier = module_specifier.clone();
        let modules = self.modules.clone();

        fn load_module(
            source_maps: SourceMapStore,
            modules: HashMap<String, ArtifactModule>,
            module_specifier: &ModuleSpecifier,
            options: &ModuleLoadOptions,
        ) -> std::result::Result<ModuleSource, deno_core::error::ModuleLoaderError> {
            let module = modules.get(module_specifier.as_str()).ok_or_else(|| {
                JsErrorBox::generic(format!(
                    "module not found in built artifact: {}",
                    module_specifier
                ))
            })?;

            let media_type = artifact_media_type(module.media_type.clone());
            let module_type = match media_type {
                MediaType::Json => ModuleType::Json,
                _ => ModuleType::JavaScript,
            };

            if module_type == ModuleType::Json
                && options.requested_module_type != deno_core::RequestedModuleType::Json
            {
                return Err(JsErrorBox::generic(
                    "attempted to load JSON module without `with { type: \"json\" }`",
                )
                .into());
            }

            let source = transpile_module_source(
                module_specifier,
                media_type,
                module.source.clone(),
                Some(&source_maps),
            )?;

            Ok(ModuleSource::new(
                module_type,
                ModuleSourceCode::String(source.into()),
                module_specifier,
                None,
            ))
        }

        ModuleLoadResponse::Sync(load_module(source_maps, modules, &module_specifier, &options))
    }

    fn get_source_map(&self, specifier: &str) -> Option<Cow<'_, [u8]>> {
        self.source_maps
            .borrow()
            .get(specifier)
            .map(|value| value.clone().into())
    }
}

pub struct JsIsolate {
    runtime: JsRuntime,
    /// True when the user module called `Deno.serve()` during module init,
    /// meaning the isolate acts as a long-running HTTP app, not a one-shot handler.
    pub is_server_mode: bool,
}

impl JsIsolate {
    pub fn new(user_code: &str, _isolate_id: usize) -> Result<Self> {
        Self::new_internal(user_code, prepare_user_code(user_code))
    }

    /// Variant used by `flux run` / `--script-mode`.  Accepts plain top-level
    /// scripts (no `export default` required) while still wiring up the handler
    /// global when `export default` IS present.
    pub fn new_for_run(user_code: &str) -> Result<Self> {
        Self::new_internal(user_code, prepare_run_code(user_code))
    }

    /// Variant used by `flux run` when loading a real JS/TS module entry.
    /// Supports relative ESM imports and TypeScript transpilation on demand.
    pub async fn new_for_run_entry(entry: &Path) -> Result<Self> {
        let source_maps = Rc::new(RefCell::new(HashMap::new()));
        let mut runtime = JsRuntime::new(RuntimeOptions {
            module_loader: Some(Rc::new(TypescriptModuleLoader {
                source_maps,
            })),
            extensions: vec![flux_runtime_ext::init()],
            create_params: Some(
                deno_core::v8::CreateParams::default()
                    .heap_limits(0, V8_HEAP_LIMIT),
            ),
            ..Default::default()
        });

        {
            let state = runtime.op_state();
            let mut state = state.borrow_mut();
            state.put::<RuntimeStateMap>(HashMap::new());
        }

        runtime
            .execute_script("flux:bootstrap_fetch", bootstrap_fetch_js())
            .context("failed to install fetch interceptor")?;

        let main_module = resolve_path(
            entry.to_str().ok_or_else(|| anyhow::anyhow!("invalid entry path: {}", entry.display()))?,
            &std::env::current_dir().context("failed to get current working directory")?,
        )
        .with_context(|| format!("failed to resolve module specifier for {}", entry.display()))?;

        let entry_source = std::fs::read_to_string(entry)
            .with_context(|| format!("failed to read {}", entry.display()))?;
        let transformed_entry = prepare_run_code(&entry_source);
        let transformed_entry = transpile_module_source(
            &main_module,
            MediaType::from_path(entry),
            transformed_entry,
            None,
        )
        .context("failed to transpile entry module")?;

        let module_id = runtime
            .load_main_es_module_from_code(&main_module, transformed_entry)
            .await
            .context("failed to load user module")?;
        let evaluation = runtime.mod_evaluate(module_id);
        tokio::time::timeout(
            EXECUTION_TIMEOUT,
            runtime.run_event_loop(Default::default()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("module initialization timed out after {EXECUTION_TIMEOUT:?}"))?
        .context("event loop error during module initialization")?;
        evaluation
            .await
            .context("failed to evaluate user module")?;

        let is_server_mode = {
            let probe = runtime
                .execute_script(
                    "flux:probe_server_mode",
                    "typeof globalThis.__flux_net_handler === 'function'",
                )
                .context("failed to probe server mode")?;
            deno_core::scope!(scope, &mut runtime);
            let local = deno_core::v8::Local::new(scope, probe);
            local.is_true()
        };

        Ok(Self { runtime, is_server_mode })
    }

    pub async fn new_from_artifact(artifact: &FluxBuildArtifact) -> Result<Self> {
        let source_maps = Rc::new(RefCell::new(HashMap::new()));
        let modules = artifact
            .modules
            .iter()
            .cloned()
            .map(|module| (module.specifier.clone(), module))
            .collect::<HashMap<_, _>>();
        let mut runtime = JsRuntime::new(RuntimeOptions {
            module_loader: Some(Rc::new(ArtifactModuleLoader {
                source_maps,
                modules,
            })),
            extensions: vec![flux_runtime_ext::init()],
            create_params: Some(
                deno_core::v8::CreateParams::default()
                    .heap_limits(0, V8_HEAP_LIMIT),
            ),
            ..Default::default()
        });

        {
            let state = runtime.op_state();
            let mut state = state.borrow_mut();
            state.put::<RuntimeStateMap>(HashMap::new());
        }

        runtime
            .execute_script("flux:bootstrap_fetch", bootstrap_fetch_js())
            .context("failed to install fetch interceptor")?;

        let entry_module = artifact
            .modules
            .iter()
            .find(|module| module.specifier == artifact.entry_specifier)
            .ok_or_else(|| anyhow::anyhow!("entry module missing from built artifact"))?;
        let main_module = Url::parse(&artifact.entry_specifier)
            .with_context(|| format!("invalid entry module specifier: {}", artifact.entry_specifier))?;
        let transformed_entry = prepare_user_code(&entry_module.source);
        let transformed_entry = transpile_module_source(
            &main_module,
            artifact_media_type(entry_module.media_type.clone()),
            transformed_entry,
            None,
        )
        .context("failed to transpile built artifact entry module")?;

        let module_id = runtime
            .load_main_es_module_from_code(&main_module, transformed_entry)
            .await
            .context("failed to load built artifact entry module")?;
        let evaluation = runtime.mod_evaluate(module_id);
        tokio::time::timeout(
            EXECUTION_TIMEOUT,
            runtime.run_event_loop(Default::default()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("module initialization timed out after {EXECUTION_TIMEOUT:?}"))?
        .context("event loop error during artifact module initialization")?;
        evaluation
            .await
            .context("failed to evaluate built artifact entry module")?;

        let is_server_mode = {
            let probe = runtime
                .execute_script(
                    "flux:probe_server_mode",
                    "typeof globalThis.__flux_net_handler === 'function'",
                )
                .context("failed to probe server mode")?;
            deno_core::scope!(scope, &mut runtime);
            let local = deno_core::v8::Local::new(scope, probe);
            local.is_true()
        };

        Ok(Self { runtime, is_server_mode })
    }

    fn new_internal(_user_code: &str, prepared: String) -> Result<Self> {
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![flux_runtime_ext::init()],
            create_params: Some(
                deno_core::v8::CreateParams::default()
                    .heap_limits(0, V8_HEAP_LIMIT),
            ),
            ..Default::default()
        });

        // Seed OpState with an empty execution-state map and the HTTP client.
        {
            let state = runtime.op_state();
            let mut state = state.borrow_mut();
            state.put::<RuntimeStateMap>(HashMap::new());
        }

        runtime
            .execute_script("flux:bootstrap_fetch", bootstrap_fetch_js())
            .context("failed to install fetch interceptor")?;

        runtime
            .execute_script("flux:user_code", prepared)
            .context("failed to load user code")?;

        // Check if the module called Deno.serve() during init.
        // In the new model, server-mode detection uses a bootstrap execution slot.
        let is_server_mode = {
            let state = runtime.op_state();
            let state = state.borrow();
            // Deno.serve wires up __flux_net_handler; check for it instead of OpState.
            // (no state slot exists yet — we check the JS side via a script)
            drop(state);
            let probe = runtime
                .execute_script(
                    "flux:probe_server_mode",
                    "typeof globalThis.__flux_net_handler === 'function'",
                )
                .context("failed to probe server mode")?;
            deno_core::scope!(scope, &mut runtime);
            let local = deno_core::v8::Local::new(scope, probe);
            local.is_true()
        };

        Ok(Self { runtime, is_server_mode })
    }

    /// Dispatch a single HTTP request into a server-mode isolate.  The JS
    /// `__flux_dispatch_request` shim feeds the request through the registered
    /// Hono / Express handler, which calls `op_net_respond` when done.
    pub async fn dispatch_request(
        &mut self,
        context: ExecutionContext,
        req: NetRequest,
    ) -> Result<NetRequestExecution> {
        let execution_id = context.execution_id.clone();
        let request_id = context.request_id.clone();

        // Register a state slot for this request.
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let map = state.borrow_mut::<RuntimeStateMap>();
            map.insert(execution_id.clone(), RuntimeExecutionState {
                context,
                call_index: 0,
                checkpoints: Vec::new(),
                recorded: HashMap::new(),
                recorded_now_ms: None,
                logs: Vec::new(),
                recorded_random: Vec::new(),
                random_index: 0,
                recorded_uuids: Vec::new(),
                uuid_index: 0,
                is_server_mode: true,
                pending_responses: HashMap::new(),
                postgres_sessions: HashMap::new(),
                next_postgres_session_id: 0,
            });
        }

        // Inject execution_id so the JS shim can thread it through all ops.
        let script = format!(
            "globalThis.__FLUX_EXECUTION_ID__ = {};\n\
             globalThis.__flux_dispatch_request({}, {}, {}, {}, {});",
            serde_json::to_string(&execution_id).unwrap(),
            serde_json::to_string(&req.req_id).unwrap(),
            serde_json::to_string(&req.method).unwrap(),
            serde_json::to_string(&req.url).unwrap(),
            serde_json::to_string(&req.headers_json).unwrap(),
            serde_json::to_string(&req.body).unwrap(),
        );

        self.runtime
            .execute_script("flux:dispatch", script)
            .context("failed to dispatch net request")?;

        tokio::time::timeout(
            EXECUTION_TIMEOUT,
            self.runtime.run_event_loop(Default::default()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("server-mode request timed out after {EXECUTION_TIMEOUT:?}"))?
        .context("event loop failed during request dispatch")?;

        let state = self.runtime.op_state();
        let mut state = state.borrow_mut();
        let map = state.borrow_mut::<RuntimeStateMap>();
        let exec = map
            .remove(&execution_id)
            .ok_or_else(|| anyhow::anyhow!("state slot missing for execution {execution_id}"))?;
        let response = exec
            .pending_responses
            .into_values()
            .next()
            .ok_or_else(|| anyhow::anyhow!("handler did not call op_net_respond for req {} (request_id={})", req.req_id, request_id))?;

        Ok(NetRequestExecution {
            response,
            checkpoints: exec.checkpoints,
            logs: exec.logs,
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
        self
            .execute_handler_with_recorded(payload, context, recorded_checkpoints, true)
            .await
    }

    async fn execute_handler_with_recorded(
        &mut self,
        payload: serde_json::Value,
        context: ExecutionContext,
        recorded_checkpoints: Vec<FetchCheckpoint>,
        wrap_payload_in_input: bool,
    ) -> Result<JsExecutionOutput> {
        let execution_id = context.execution_id.clone();
        let recorded: HashMap<u32, FetchCheckpoint> = recorded_checkpoints
            .into_iter()
            .map(|cp| (cp.call_index, cp))
            .collect();

        // Register the state slot before injecting JS.
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let map = state.borrow_mut::<RuntimeStateMap>();
            map.insert(execution_id.clone(), RuntimeExecutionState {
                context,
                call_index: 0,
                checkpoints: Vec::new(),
                recorded,
                recorded_now_ms: None,
                logs: Vec::new(),
                recorded_random: Vec::new(),
                random_index: 0,
                recorded_uuids: Vec::new(),
                uuid_index: 0,
                is_server_mode: false,
                pending_responses: HashMap::new(),
                postgres_sessions: HashMap::new(),
                next_postgres_session_id: 0,
            });
        }

        let eid_json = serde_json::to_string(&execution_id).context("failed to encode execution_id")?;
        let payload_json = serde_json::to_string(&payload).context("failed to encode payload")?;
        let handler_arg = if wrap_payload_in_input {
            format!("{{ input: {payload_json}, ctx }}")
        } else {
            payload_json.clone()
        };
        let invoke = format!(
            "(async () => {{\n\
               const __eid = {eid};\n\
               globalThis.__FLUX_EXECUTION_ID__ = __eid;\n\
               globalThis.__flux_last_result = globalThis.__flux_last_result || {{}};\n\
               globalThis.__flux_last_result[__eid] = null;\n\
               globalThis.__flux_last_error = globalThis.__flux_last_error || {{}};\n\
               globalThis.__flux_last_error[__eid] = null;\n\
               try {{\n\
                 const ctx = {{}};\n\
                                 const result = await globalThis.__flux_user_handler({handler_arg});\n\
                 globalThis.__flux_last_result[__eid] = result ?? null;\n\
               }} catch (err) {{\n\
                 globalThis.__flux_last_error[__eid] = String(err && err.stack ? err.stack : err);\n\
               }}\n\
             }})();",
            eid = eid_json,
                        handler_arg = handler_arg,
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

        let result_script = format!(
            "JSON.stringify({{ result: (globalThis.__flux_last_result || {{}})[{eid}] ?? null, error: (globalThis.__flux_last_error || {{}})[{eid}] ?? null }})",
            eid = eid_json,
        );

        let result_value = self
            .runtime
            .execute_script("flux:result", result_script)
            .context("failed to read handler result")?;

        let raw: String = {
            deno_core::scope!(scope, &mut self.runtime);
            let local = deno_core::v8::Local::new(scope, result_value);
            deno_core::serde_v8::from_v8(scope, local)
                .context("failed to deserialize handler result")?
        };

        let envelope: serde_json::Value = serde_json::from_str(&raw)
            .context("handler result envelope is not valid JSON")?;

        let (checkpoints, logs) = {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let map = state.borrow_mut::<RuntimeStateMap>();
            match map.remove(&execution_id) {
                Some(execution) => (execution.checkpoints, execution.logs),
                None => (Vec::new(), Vec::new()),
            }
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
            logs,
        })
    }

    /// Run the entry module in script mode — the `flux run` equivalent of
    /// `node script.js`.
    ///
    /// Two sub-modes, selected automatically:
    ///
    /// **Handler mode** — the file exports a default function.  Flux calls it
    /// with `input` (defaults to `{}`), drains the event loop, and returns the
    /// output and any captured logs.
    ///
    /// **Top-level mode** — no exported handler.  `input` is ignored.  Flux
    /// simply drains the event loop so that top-level `await` and `setTimeout`
    /// promises resolve, then returns the captured logs.
    ///
    /// In both cases, `console.log/warn/error` output is streamed to
    /// stdout/stderr by `op_console` AND collected in the returned log vec.
    pub async fn run_script(&mut self, input: serde_json::Value) -> Result<(Option<serde_json::Value>, Vec<LogEntry>)> {
        // Check whether the module registered a handler during initialisation.
        let has_handler = {
            let check = self.runtime
                .execute_script(
                    "flux:check_handler",
                    "typeof globalThis.__flux_user_handler === 'function'",
                )
                .context("failed to check for exported handler")?;
            deno_core::scope!(scope, &mut self.runtime);
            let local = deno_core::v8::Local::new(scope, check);
            local.is_true()
        };

        if has_handler {
            let context = ExecutionContext::new("__run__");
            let output = self
                .execute_handler_with_recorded(input, context, Vec::new(), false)
                .await?;
            if let Some(ref err) = output.error {
                eprintln!("error: {err}");
            }
            return Ok((Some(output.output), output.logs));
        }

        // Top-level mode: register a transient state slot so ops don't panic,
        // then drain the event loop.
        let execution_id = "__script__".to_string();
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let map = state.borrow_mut::<RuntimeStateMap>();
            map.insert(execution_id.clone(), RuntimeExecutionState {
                context: ExecutionContext::new("__script__"),
                call_index: 0,
                checkpoints: Vec::new(),
                recorded: HashMap::new(),
                recorded_now_ms: None,
                logs: Vec::new(),
                recorded_random: Vec::new(),
                random_index: 0,
                recorded_uuids: Vec::new(),
                uuid_index: 0,
                is_server_mode: false,
                pending_responses: HashMap::new(),
                postgres_sessions: HashMap::new(),
                next_postgres_session_id: 0,
            });
        }

        // Tell bootstrap JS which execution_id to use for top-level ops.
        let eid_json = serde_json::to_string(&execution_id).unwrap();
        self.runtime
            .execute_script("flux:set_script_eid", format!("globalThis.__FLUX_EXECUTION_ID__ = {eid_json};"))
            .context("failed to set execution_id")?;

        tokio::time::timeout(
            EXECUTION_TIMEOUT,
            self.runtime.run_event_loop(Default::default()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("script timed out after {EXECUTION_TIMEOUT:?}"))?
        .context("event loop error during script execution")?;

        let state = self.runtime.op_state();
        let mut state = state.borrow_mut();
        let map = state.borrow_mut::<RuntimeStateMap>();
        let logs = map
            .remove(&execution_id)
            .map(|e| e.logs)
            .unwrap_or_default();
        Ok((None, logs))
    }
}

fn bootstrap_fetch_js() -> &'static str {
    r#"
// Flux provides a small buffered web-platform shim here. Transport, time,
// randomness, logging, and server dispatch remain owned by Flux so recording
// and replay stay deterministic.

// ── Execution ID accessor ────────────────────────────────────────────────────
// All op wrappers below use this helper so each concurrent execution threads
// its own ID through the Rust ops, which index into the per-execution HashMap.
function __flux_eid() {
  return globalThis.__FLUX_EXECUTION_ID__ || "__unknown__";
}

function __fluxToUSVString(value) {
    const input = String(value);
    let output = "";

    for (let index = 0; index < input.length; index++) {
        const codeUnit = input.charCodeAt(index);

        if (codeUnit >= 0xD800 && codeUnit <= 0xDBFF) {
            const nextCodeUnit = index + 1 < input.length ? input.charCodeAt(index + 1) : 0;
            if (nextCodeUnit >= 0xDC00 && nextCodeUnit <= 0xDFFF) {
                output += input[index] + input[index + 1];
                index += 1;
            } else {
                output += "\uFFFD";
            }
            continue;
        }

        if (codeUnit >= 0xDC00 && codeUnit <= 0xDFFF) {
            output += "\uFFFD";
            continue;
        }

        output += input[index];
    }

    return output;
}

function __fluxEncodeUtf8(input) {
    const normalized = __fluxToUSVString(input);
    const bytes = [];

    for (let index = 0; index < normalized.length; index++) {
        const codePoint = normalized.codePointAt(index);
        if (codePoint > 0xFFFF) {
            index += 1;
        }

        if (codePoint <= 0x7F) {
            bytes.push(codePoint);
        } else if (codePoint <= 0x7FF) {
            bytes.push(
                0xC0 | (codePoint >> 6),
                0x80 | (codePoint & 0x3F),
            );
        } else if (codePoint <= 0xFFFF) {
            bytes.push(
                0xE0 | (codePoint >> 12),
                0x80 | ((codePoint >> 6) & 0x3F),
                0x80 | (codePoint & 0x3F),
            );
        } else {
            bytes.push(
                0xF0 | (codePoint >> 18),
                0x80 | ((codePoint >> 12) & 0x3F),
                0x80 | ((codePoint >> 6) & 0x3F),
                0x80 | (codePoint & 0x3F),
            );
        }
    }

    return new Uint8Array(bytes);
}

function __fluxDecodeUtf8(input) {
    const bytes = input instanceof Uint8Array ? input : new Uint8Array(input ?? []);
    let output = "";

    for (let index = 0; index < bytes.length;) {
        const byte1 = bytes[index];

        if (byte1 <= 0x7F) {
            output += String.fromCodePoint(byte1);
            index += 1;
            continue;
        }

        let needed = 0;
        let codePoint = 0;
        let minimum = 0;

        if (byte1 >= 0xC2 && byte1 <= 0xDF) {
            needed = 1;
            codePoint = byte1 & 0x1F;
            minimum = 0x80;
        } else if (byte1 >= 0xE0 && byte1 <= 0xEF) {
            needed = 2;
            codePoint = byte1 & 0x0F;
            minimum = 0x800;
        } else if (byte1 >= 0xF0 && byte1 <= 0xF4) {
            needed = 3;
            codePoint = byte1 & 0x07;
            minimum = 0x10000;
        } else {
            output += "\uFFFD";
            index += 1;
            continue;
        }

        if (index + needed >= bytes.length) {
            output += "\uFFFD";
            index += 1;
            continue;
        }

        let valid = true;
        for (let offset = 1; offset <= needed; offset++) {
            const nextByte = bytes[index + offset];
            if ((nextByte & 0xC0) !== 0x80) {
                valid = false;
                break;
            }
            codePoint = (codePoint << 6) | (nextByte & 0x3F);
        }

        if (
            !valid ||
            codePoint < minimum ||
            codePoint > 0x10FFFF ||
            (codePoint >= 0xD800 && codePoint <= 0xDFFF)
        ) {
            output += "\uFFFD";
            index += 1;
            continue;
        }

        output += String.fromCodePoint(codePoint);
        index += needed + 1;
    }

    return output;
}

class TextEncoder {
    encode(input = "") {
        return __fluxEncodeUtf8(input);
    }
}

class TextDecoder {
    decode(input = undefined) {
        if (input == null) return "";
        return __fluxDecodeUtf8(input);
    }
}

globalThis.TextEncoder = globalThis.TextEncoder || TextEncoder;
globalThis.TextDecoder = globalThis.TextDecoder || TextDecoder;

const __fluxEncoder = new globalThis.TextEncoder();
const __fluxDecoder = new globalThis.TextDecoder();

function __fluxNormalizeHeaderName(name) {
    return String(name).toLowerCase();
}

function __fluxBodyToText(body) {
    if (body == null) return null;
    if (typeof body === "string") return body;
    if (body instanceof Uint8Array) return __fluxDecoder.decode(body);
    return String(body);
}

function __fluxCreateBodyState(bodyText) {
    return {
        bodyText,
        used: false,
        locked: false,
        emitted: false,
    };
}

function __fluxConsumeBodyText(state) {
    if (state.used) {
        throw new TypeError("Body already consumed");
    }
    state.used = true;
    state.emitted = true;
    return state.bodyText ?? "";
}

function __fluxCreateBodyStream(state) {
    return {
        getReader() {
            if (state.locked) {
                throw new TypeError("ReadableStream is locked");
            }
            state.locked = true;
            return {
                async read() {
                    if (state.emitted || state.bodyText === null) {
                        state.used = true;
                        state.emitted = true;
                        return { value: undefined, done: true };
                    }
                    state.used = true;
                    state.emitted = true;
                    return {
                        value: __fluxEncoder.encode(state.bodyText),
                        done: false,
                    };
                },
                releaseLock() {
                    state.locked = false;
                },
                async cancel() {
                    state.used = true;
                    state.emitted = true;
                    state.locked = false;
                },
            };
        },
    };
}

function __fluxDecodeFormComponent(value) {
    const normalized = String(value).replace(/\+/g, " ");
    const bytes = [];

    for (let index = 0; index < normalized.length; index++) {
        const codePoint = normalized.codePointAt(index);
        const char = String.fromCodePoint(codePoint);
        if (codePoint > 0xFFFF) {
            index += 1;
        }
        if (
            char === "%" &&
            index + 2 < normalized.length &&
            /[0-9a-fA-F]{2}/.test(normalized.slice(index + 1, index + 3))
        ) {
            bytes.push(parseInt(normalized.slice(index + 1, index + 3), 16));
            index += 2;
            continue;
        }

        const encoded = __fluxEncoder.encode(char);
        for (const byte of encoded) {
            bytes.push(byte);
        }
    }

    return __fluxDecoder.decode(new Uint8Array(bytes));
}

function __fluxEncodeFormComponent(value) {
    return encodeURIComponent(__fluxToUSVString(value)).replace(/%20/g, "+");
}

function __fluxAbortError(reason = undefined) {
    if (reason instanceof DOMException && reason.name === "AbortError") {
        return reason;
    }
    if (reason instanceof Error) {
        return reason;
    }
    if (reason === undefined) {
        return new DOMException("This operation was aborted", "AbortError");
    }
    return new DOMException(String(reason), "AbortError");
}

function __fluxInvokeEventListener(listener, event) {
    if (typeof listener === "function") {
        listener.call(event.currentTarget, event);
        return;
    }
    if (listener && typeof listener.handleEvent === "function") {
        listener.handleEvent(event);
    }
}

const __fluxDomExceptionCodeByName = {
    IndexSizeError: 1,
    DOMStringSizeError: 2,
    HierarchyRequestError: 3,
    WrongDocumentError: 4,
    InvalidCharacterError: 5,
    NoDataAllowedError: 6,
    NoModificationAllowedError: 7,
    NotFoundError: 8,
    NotSupportedError: 9,
    InUseAttributeError: 10,
    InvalidStateError: 11,
    SyntaxError: 12,
    InvalidModificationError: 13,
    NamespaceError: 14,
    InvalidAccessError: 15,
    ValidationError: 16,
    TypeMismatchError: 17,
    SecurityError: 18,
    NetworkError: 19,
    AbortError: 20,
    URLMismatchError: 21,
    QuotaExceededError: 22,
    TimeoutError: 23,
    InvalidNodeTypeError: 24,
    DataCloneError: 25,
};

const __fluxDomExceptionLegacyConstants = [
    ["INDEX_SIZE_ERR", 1],
    ["DOMSTRING_SIZE_ERR", 2],
    ["HIERARCHY_REQUEST_ERR", 3],
    ["WRONG_DOCUMENT_ERR", 4],
    ["INVALID_CHARACTER_ERR", 5],
    ["NO_DATA_ALLOWED_ERR", 6],
    ["NO_MODIFICATION_ALLOWED_ERR", 7],
    ["NOT_FOUND_ERR", 8],
    ["NOT_SUPPORTED_ERR", 9],
    ["INUSE_ATTRIBUTE_ERR", 10],
    ["INVALID_STATE_ERR", 11],
    ["SYNTAX_ERR", 12],
    ["INVALID_MODIFICATION_ERR", 13],
    ["NAMESPACE_ERR", 14],
    ["INVALID_ACCESS_ERR", 15],
    ["VALIDATION_ERR", 16],
    ["TYPE_MISMATCH_ERR", 17],
    ["SECURITY_ERR", 18],
    ["NETWORK_ERR", 19],
    ["ABORT_ERR", 20],
    ["URL_MISMATCH_ERR", 21],
    ["QUOTA_EXCEEDED_ERR", 22],
    ["TIMEOUT_ERR", 23],
    ["INVALID_NODE_TYPE_ERR", 24],
    ["DATA_CLONE_ERR", 25],
];

class DOMException extends Error {
    constructor(message = "", name = "Error") {
        super(String(message));
        this.name = String(name);
        this.code = __fluxDomExceptionCodeByName[this.name] || 0;
    }
}

class AbortSignal {
    constructor() {
        this.aborted = false;
        this.reason = undefined;
        this.onabort = null;
        this._listeners = new Set();
    }

    addEventListener(type, listener) {
        if (type !== "abort" || listener == null) return;
        if (this.aborted) {
            __fluxInvokeEventListener(listener, {
                type: "abort",
                target: this,
                currentTarget: this,
            });
            return;
        }
        this._listeners.add(listener);
    }

    removeEventListener(type, listener) {
        if (type !== "abort" || listener == null) return;
        this._listeners.delete(listener);
    }

    throwIfAborted() {
        if (this.aborted) {
            throw __fluxAbortError(this.reason);
        }
    }

    _abort(reason = undefined) {
        if (this.aborted) return;
        this.aborted = true;
        this.reason = reason === undefined
            ? new DOMException("This operation was aborted", "AbortError")
            : reason;

        const event = {
            type: "abort",
            target: this,
            currentTarget: this,
        };

        if (typeof this.onabort === "function") {
            __fluxInvokeEventListener(this.onabort, event);
        }

        for (const listener of [...this._listeners]) {
            __fluxInvokeEventListener(listener, event);
        }
        this._listeners.clear();
    }

    static abort(reason = undefined) {
        const controller = new AbortController();
        controller.abort(reason);
        return controller.signal;
    }
}

class AbortController {
    constructor() {
        this.signal = new AbortSignal();
    }

    abort(reason = undefined) {
        this.signal._abort(reason);
    }
}

for (const [constantName, constantValue] of __fluxDomExceptionLegacyConstants) {
    Object.defineProperty(DOMException, constantName, {
        value: constantValue,
        enumerable: true,
        configurable: false,
        writable: false,
    });
    Object.defineProperty(DOMException.prototype, constantName, {
        value: constantValue,
        enumerable: true,
        configurable: false,
        writable: false,
    });
}

class URLSearchParams {
    constructor(init = "", update = null) {
        this._pairs = [];
        this._update = typeof update === "function" ? update : null;
        this._suspendUpdates = 0;

        this._withUpdatesSuspended(() => {
            this._initialize(init);
        });
    }

    _initialize(init) {
        if (init == null) {
            return;
        }

        if (typeof init === "string") {
            const query = init.startsWith("?") ? init.slice(1) : init;
            if (!query) return;
            for (const pair of query.split("&")) {
                if (!pair) continue;
                const separatorIndex = pair.indexOf("=");
                const rawKey = separatorIndex === -1 ? pair : pair.slice(0, separatorIndex);
                const rawValue = separatorIndex === -1 ? "" : pair.slice(separatorIndex + 1);
                this._appendPair(__fluxDecodeFormComponent(rawKey), __fluxDecodeFormComponent(rawValue));
            }
            return;
        }

        if (typeof init === "object" || typeof init === "function") {
            if (init === DOMException.prototype) {
                throw new TypeError("Invalid URLSearchParams initializer");
            }

            const iterator = init[Symbol.iterator];
            if (typeof iterator === "function") {
                for (const entry of iterator.call(init)) {
                    const pair = Array.from(entry || []);
                    if (pair.length !== 2) {
                        throw new TypeError("Expected sequence pair");
                    }
                    this._appendPair(pair[0], pair[1]);
                }
                return;
            }

            const normalizedRecord = Object.create(null);
            for (const [key, value] of Object.entries(init)) {
                normalizedRecord[__fluxToUSVString(key)] = __fluxToUSVString(value);
            }

            for (const [key, value] of Object.entries(normalizedRecord)) {
                this._appendPair(key, value);
            }
        }
    }

    _appendPair(name, value) {
        this._pairs.push([__fluxToUSVString(name), __fluxToUSVString(value)]);
    }

    _withUpdatesSuspended(callback) {
        this._suspendUpdates += 1;
        try {
            return callback();
        } finally {
            this._suspendUpdates -= 1;
        }
    }

    _replacePairs(nextPairs) {
        this._pairs.splice(0, this._pairs.length, ...nextPairs.map(([key, value]) => [String(key), String(value)]));
    }

    _resetFromQuery(query) {
        this._withUpdatesSuspended(() => {
            const nextParams = new URLSearchParams(query);
            this._replacePairs(nextParams._pairs);
        });
    }

    append(name, value) {
        this._appendPair(name, value);
        this._commit();
    }

    get(name) {
        const key = String(name);
        const match = this._pairs.find(([candidate]) => candidate === key);
        return match ? match[1] : null;
    }

    getAll(name) {
        const key = String(name);
        return this._pairs
            .filter(([candidate]) => candidate === key)
            .map(([, value]) => value);
    }

    has(name, value = undefined) {
        const key = String(name);
        if (arguments.length > 1 && value !== undefined) {
            const expected = String(value);
            return this._pairs.some(([candidate, currentValue]) => candidate === key && currentValue === expected);
        }
        return this._pairs.some(([candidate]) => candidate === key);
    }

    set(name, value) {
        const key = __fluxToUSVString(name);
        const nextValue = __fluxToUSVString(value);
        let replaced = false;

        for (let index = 0; index < this._pairs.length;) {
            const [candidate] = this._pairs[index];
            if (candidate !== key) {
                index += 1;
                continue;
            }

            if (!replaced) {
                this._pairs[index][1] = nextValue;
                replaced = true;
                index += 1;
            } else {
                this._pairs.splice(index, 1);
            }
        }

        if (!replaced) {
            this._appendPair(key, nextValue);
        }

        this._commit();
    }

    delete(name, value = undefined) {
        const key = __fluxToUSVString(name);
        const expected = arguments.length > 1 && value !== undefined ? __fluxToUSVString(value) : null;
        for (let index = 0; index < this._pairs.length;) {
            const [candidate, currentValue] = this._pairs[index];
            const matches = arguments.length > 1 && value !== undefined
                ? candidate === key && currentValue === expected
                : candidate === key;

            if (matches) {
                this._pairs.splice(index, 1);
            } else {
                index += 1;
            }
        }
        this._commit();
    }

    forEach(callback, thisArg = undefined) {
        for (const [key, value] of this) {
            callback.call(thisArg, value, key, this);
        }
    }

    keys() {
        const entries = this.entries();
        return {
            [Symbol.iterator]() {
                return this;
            },
            next() {
                const nextEntry = entries.next();
                if (nextEntry.done) return nextEntry;
                return { value: nextEntry.value[0], done: false };
            },
        };
    }

    values() {
        const entries = this.entries();
        return {
            [Symbol.iterator]() {
                return this;
            },
            next() {
                const nextEntry = entries.next();
                if (nextEntry.done) return nextEntry;
                return { value: nextEntry.value[1], done: false };
            },
        };
    }

    sort() {
        this._pairs.sort(([leftKey], [rightKey]) => {
            if (leftKey < rightKey) return -1;
            if (leftKey > rightKey) return 1;
            return 0;
        });
        this._commit();
    }

    get size() {
        return this._pairs.length;
    }

    _commit() {
        if (this._update && this._suspendUpdates === 0) {
            this._update(this.toString());
        }
    }

    entries() {
        const params = this;
        let index = 0;
        return {
            [Symbol.iterator]() {
                return this;
            },
            next() {
                if (index >= params._pairs.length) {
                    return { value: undefined, done: true };
                }
                const value = params._pairs[index];
                index += 1;
                return { value, done: false };
            },
        };
    }

    [Symbol.iterator]() {
        return this.entries();
    }

    toString() {
        return this._pairs
            .map(([key, value]) => `${__fluxEncodeFormComponent(key)}=${__fluxEncodeFormComponent(value)}`)
            .join("&");
    }
}

class FormData {
    constructor() {
        this._entries = [];
    }

    append(name, value) {
        this._entries.push([__fluxToUSVString(name), __fluxToUSVString(value)]);
    }

    has(name) {
        const key = __fluxToUSVString(name);
        return this._entries.some(([candidate]) => candidate === key);
    }

    entries() {
        return this._entries[Symbol.iterator]();
    }

    [Symbol.iterator]() {
        return this.entries();
    }
}

function __fluxParseFormData(text) {
    const params = new URLSearchParams(text);
    const formData = new FormData();
    for (const [key, value] of params) {
        formData.append(key, value);
    }
    return formData;
}

class URL {
    constructor(input, base = undefined) {
        const parsed = Deno.core.ops.op_flux_parse_url(String(input), base == null ? "" : String(base));
        this._applyParsed(parsed);
        const searchParams = new URLSearchParams(this._search, (nextSearchParams) => {
            this.search = nextSearchParams ? `?${nextSearchParams}` : "";
        });
        Object.defineProperty(this, "searchParams", {
            value: searchParams,
            writable: false,
            enumerable: true,
            configurable: true,
        });
    }

    static canParse(input, base = undefined) {
        try {
            new URL(input, base);
            return true;
        } catch {
            return false;
        }
    }

    static parse(input, base = undefined) {
        try {
            return new URL(input, base);
        } catch {
            return null;
        }
    }

    _applyParsed(parsed) {
        this._href = parsed.href;
        this._origin = parsed.origin;
        this._protocol = parsed.protocol;
        this._username = parsed.username;
        this._password = parsed.password ?? "";
        this._host = parsed.host;
        this._hostname = parsed.hostname;
        this._port = parsed.port;
        this._pathname = parsed.pathname;
        this._search = parsed.search;
        this._hash = parsed.hash;
    }

    _reparseFrom(parts) {
        const credentials = parts.username
            ? `${parts.username}${parts.password ? `:${parts.password}` : ""}@`
            : "";
        const authority = parts.host ? `//${credentials}${parts.host}` : "";
        const nextHref = `${parts.protocol}${authority}${parts.pathname}${parts.search}${parts.hash}`;
        this._applyParsed(Deno.core.ops.op_flux_parse_url(nextHref, ""));
        if (this.searchParams) {
            this.searchParams._resetFromQuery(this._search);
        }
    }

    get href() {
        return this._href;
    }

    set href(value) {
        this._applyParsed(Deno.core.ops.op_flux_parse_url(String(value), ""));
        if (this.searchParams) {
            this.searchParams._resetFromQuery(this._search);
        }
    }

    get origin() {
        return this._origin;
    }

    get protocol() {
        return this._protocol;
    }

    set protocol(value) {
        this._reparseFrom({
            protocol: String(value),
            username: this._username,
            password: this._password,
            host: this._host,
            pathname: this._pathname,
            search: this._search,
            hash: this._hash,
        });
    }

    get username() {
        return this._username;
    }

    set username(value) {
        this._reparseFrom({
            protocol: this._protocol,
            username: String(value),
            password: this._password,
            host: this._host,
            pathname: this._pathname,
            search: this._search,
            hash: this._hash,
        });
    }

    get password() {
        return this._password;
    }

    set password(value) {
        this._reparseFrom({
            protocol: this._protocol,
            username: this._username,
            password: String(value),
            host: this._host,
            pathname: this._pathname,
            search: this._search,
            hash: this._hash,
        });
    }

    get host() {
        return this._host;
    }

    get hostname() {
        return this._hostname;
    }

    get port() {
        return this._port;
    }

    get pathname() {
        return this._pathname;
    }

    get search() {
        return this._search;
    }

    set search(value) {
        const nextSearch = value === "" ? "" : String(value).startsWith("?") ? String(value) : `?${value}`;
        this._reparseFrom({
            protocol: this._protocol,
            username: this._username,
            password: this._password,
            host: this._host,
            pathname: this._pathname,
            search: nextSearch,
            hash: this._hash,
        });
    }

    get hash() {
        return this._hash;
    }

    set hash(value) {
        const nextHash = value === ""
            ? ""
            : String(value).startsWith(String.fromCharCode(35))
                ? String(value)
                : `#${value}`;
        this._reparseFrom({
            protocol: this._protocol,
            username: this._username,
            password: this._password,
            host: this._host,
            pathname: this._pathname,
            search: this._search,
            hash: nextHash,
        });
    }

    toString() {
        return this._href;
    }

    toJSON() {
        return this._href;
    }
}

class Headers {
    constructor(init = undefined) {
        this._map = new Map();
        if (init instanceof Headers) {
            for (const [key, value] of init.entries()) this.append(key, value);
            return;
        }
        if (Array.isArray(init)) {
            for (const [key, value] of init) this.append(key, value);
            return;
        }
        if (init && typeof init === "object") {
            for (const [key, value] of Object.entries(init)) this.append(key, value);
        }
    }

    append(name, value) {
        const key = __fluxNormalizeHeaderName(name);
        const existing = this._map.get(key);
        const next = String(value);
        this._map.set(key, existing ? `${existing}, ${next}` : next);
    }

    delete(name) {
        this._map.delete(__fluxNormalizeHeaderName(name));
    }

    get(name) {
        const value = this._map.get(__fluxNormalizeHeaderName(name));
        return value === undefined ? null : value;
    }

    has(name) {
        return this._map.has(__fluxNormalizeHeaderName(name));
    }

    set(name, value) {
        this._map.set(__fluxNormalizeHeaderName(name), String(value));
    }

    entries() {
        return this._map.entries();
    }

    keys() {
        return this._map.keys();
    }

    values() {
        return this._map.values();
    }

    forEach(callback, thisArg = undefined) {
        for (const [key, value] of this._map.entries()) {
            callback.call(thisArg, value, key, this);
        }
    }

    [Symbol.iterator]() {
        return this.entries();
    }
}

class Request {
    constructor(input, init = undefined) {
        const source = input instanceof Request ? input : null;
        const options = init || {};

        this.url = source ? source.url : String(input);
        this.method = String(options.method || (source ? source.method : "GET")).toUpperCase();
        this.headers = new Headers(options.headers || (source ? source.headers : undefined));
        this.signal = options.signal !== undefined
            ? options.signal
            : source
                ? source.signal
                : null;
        this._bodyState = __fluxCreateBodyState(options.body !== undefined
            ? __fluxBodyToText(options.body)
            : source
                ? source._bodyState.bodyText
                : null);
        this._bodyStream = null;
    }

    get bodyUsed() {
        return this._bodyState.used;
    }

    get body() {
        if (this._bodyState.bodyText === null) return null;
        if (!this._bodyStream) {
            this._bodyStream = __fluxCreateBodyStream(this._bodyState);
        }
        return this._bodyStream;
    }

    async text() {
        return __fluxConsumeBodyText(this._bodyState);
    }

    async json() {
        return JSON.parse(await this.text());
    }

    async formData() {
        return __fluxParseFormData(await this.text());
    }
}

class Response {
    constructor(body = null, init = undefined) {
        const options = init || {};
        this._bodyState = __fluxCreateBodyState(__fluxBodyToText(body));
        this._bodyStream = null;
        this.status = options.status ?? 200;
        this.statusText = options.statusText ?? "";
        this.headers = new Headers(options.headers);
    }

    static json(value, init = undefined) {
        const options = init || {};
        const headers = new Headers(options.headers);
        if (!headers.has("content-type")) {
            headers.set("content-type", "application/json");
        }
        return new Response(JSON.stringify(value), {
            ...options,
            headers,
        });
    }

    get ok() {
        return this.status >= 200 && this.status < 300;
    }

    get bodyUsed() {
        return this._bodyState.used;
    }

    get body() {
        if (this._bodyState.bodyText === null) return null;
        if (!this._bodyStream) {
            this._bodyStream = __fluxCreateBodyStream(this._bodyState);
        }
        return this._bodyStream;
    }

    async text() {
        return __fluxConsumeBodyText(this._bodyState);
    }

    async json() {
        return JSON.parse(await this.text());
    }

    async formData() {
        return __fluxParseFormData(await this.text());
    }

    clone() {
        if (this.bodyUsed) {
            throw new TypeError("Body already consumed");
        }
        return new Response(this._bodyState.bodyText, {
            status: this.status,
            statusText: this.statusText,
            headers: this.headers,
        });
    }
}

globalThis.URLSearchParams = globalThis.URLSearchParams || URLSearchParams;
globalThis.URL = globalThis.URL || URL;
globalThis.DOMException = globalThis.DOMException || DOMException;
globalThis.AbortSignal = globalThis.AbortSignal || AbortSignal;
globalThis.AbortController = globalThis.AbortController || AbortController;
globalThis.Headers = Headers;
globalThis.FormData = globalThis.FormData || FormData;
globalThis.Request = Request;
globalThis.Response = Response;

// ── crypto ──────────────────────────────────────────────────────────────────
if (!globalThis.crypto) globalThis.crypto = {};
globalThis.crypto.getRandomValues = (typedArray) => {
    if (!typedArray || typeof typedArray.length !== "number") {
        throw new TypeError("crypto.getRandomValues expected a typed array");
    }
    for (let i = 0; i < typedArray.length; i++) {
        typedArray[i] = Math.floor(Deno.core.ops.op_random(__flux_eid()) * 256);
    }
    return typedArray;
};
globalThis.crypto.randomUUID = () => Deno.core.ops.op_random_uuid(__flux_eid());

// ── fetch ──────────────────────────────────────────────────────────────────
globalThis.fetch = async function(input, init = undefined) {
    const request = input instanceof Request ? input : new Request(input, init);
    const signal = request.signal || null;
    if (signal) {
        if (typeof signal.throwIfAborted === "function") {
            signal.throwIfAborted();
        } else if (signal.aborted) {
            throw __fluxAbortError(signal.reason);
        }
    }
    const method = request.method || "GET";
    const headers = Object.fromEntries(request.headers.entries());
    const body = (method === "GET" || method === "HEAD") ? null : await request.text();
    const response = await Deno.core.ops.op_flux_fetch({
        execution_id: __flux_eid(),
        url: String(request.url),
        method: String(method),
        body,
        headers,
    });

    const responseBody = response.body == null
        ? null
        : typeof response.body === "string"
            ? response.body
            : JSON.stringify(response.body);

    return new Response(responseBody, {
        status: response.status,
        headers: new Headers(response.headers ?? {}),
    });
};

// ── Date.now() + new Date() ────────────────────────────────────────────────
{
  const _OrigDate = globalThis.Date;
  class PatchedDate extends _OrigDate {
    constructor(...args) {
      if (args.length === 0) {
        super(Deno.core.ops.op_flux_now(__flux_eid()));
      } else {
        super(...args);
      }
    }
  }
    PatchedDate.now = function() { return Deno.core.ops.op_flux_now(__flux_eid()); };
  globalThis.Date = PatchedDate;
}

// ── performance.now() ──────────────────────────────────────────────────────
if (globalThis.performance) {
    globalThis.performance.now = function() { return Deno.core.ops.op_flux_now(__flux_eid()); };
}

// ── console ────────────────────────────────────────────────────────────────
function _flux_fmt(...args) {
  return args.map(v => (typeof v === "string" ? v : JSON.stringify(v))).join(" ");
}
console.log   = (...a) => Deno.core.ops.op_console(__flux_eid(), _flux_fmt(...a), false);
console.info  = (...a) => Deno.core.ops.op_console(__flux_eid(), _flux_fmt(...a), false);
console.warn  = (...a) => Deno.core.ops.op_console(__flux_eid(), _flux_fmt(...a), false);
console.error = (...a) => Deno.core.ops.op_console(__flux_eid(), _flux_fmt(...a), true);
console.debug = (...a) => Deno.core.ops.op_console(__flux_eid(), _flux_fmt(...a), false);
console.assert = (condition, ...a) => {
    if (condition) return;
    const message = a.length > 0 ? `Assertion failed: ${_flux_fmt(...a)}` : "Assertion failed";
    Deno.core.ops.op_console(__flux_eid(), message, true);
};
console.trace = (...a) => {
    const message = a.length > 0 ? _flux_fmt(...a) : "Trace";
    const err = new Error(message);
    Deno.core.ops.op_console(__flux_eid(), err.stack || message, true);
};

// ── setTimeout / setInterval ────────────────────────────────────────────────
let __flux_timer_id = 1;
const __flux_active_timers = new Map();

function __flux_invoke_timer_callback(fn, args) {
    if (typeof fn === "function") {
        fn(...args);
        return;
    }
    throw new TypeError("Timer callback must be a function");
}

globalThis.setTimeout = (fn, delay, ...args) => {
    const timerId = __flux_timer_id++;
    __flux_active_timers.set(timerId, { interval: false, cancelled: false });
    const effectiveDelay = Deno.core.ops.op_timer_delay(__flux_eid(), delay ?? 0);
    queueMicrotask(() => {
        const timer = __flux_active_timers.get(timerId);
        if (!timer || timer.cancelled) return;
        __flux_active_timers.delete(timerId);
        __flux_invoke_timer_callback(fn, args);
    });
    return timerId;
};

globalThis.clearTimeout = (timerId) => {
    const timer = __flux_active_timers.get(timerId);
    if (!timer) return;
    timer.cancelled = true;
    __flux_active_timers.delete(timerId);
};

globalThis.setInterval = (fn, delay, ...args) => {
    const timerId = __flux_timer_id++;
    __flux_active_timers.set(timerId, { interval: true, cancelled: false });

    function tick() {
        const timer = __flux_active_timers.get(timerId);
        if (!timer || timer.cancelled) return;
        Deno.core.ops.op_timer_delay(__flux_eid(), delay ?? 0);
        queueMicrotask(() => {
            const nextTimer = __flux_active_timers.get(timerId);
            if (!nextTimer || nextTimer.cancelled) return;
            __flux_invoke_timer_callback(fn, args);
            tick();
        });
    }

    tick();
    return timerId;
};

globalThis.clearInterval = (timerId) => {
    const timer = __flux_active_timers.get(timerId);
    if (!timer) return;
    timer.cancelled = true;
    __flux_active_timers.delete(timerId);
};

// ── Math.random ─────────────────────────────────────────────────────────────
Math.random = () => Deno.core.ops.op_random(__flux_eid());

// ── Flux.net (deterministic outbound TCP) ───────────────────────────────────
globalThis.Flux = globalThis.Flux || {};
globalThis.Flux.postgres = globalThis.Flux.postgres || {};
globalThis.Flux.net = globalThis.Flux.net || {};
const __flux_pg_builtin_types = Object.freeze({
    DATE: 1082,
    TIMESTAMP: 1114,
    TIMESTAMPTZ: 1184,
    INTERVAL: 1186,
});
globalThis.Flux.postgres.nodePgTypes = Object.freeze({
    builtins: __flux_pg_builtin_types,
    getTypeParser(_typeId, _format) {
        return (value) => value;
    },
});
function __flux_normalize_node_pg_query(queryOrConfig, params) {
    if (typeof queryOrConfig === "string") {
        return {
            text: queryOrConfig,
            params: Array.isArray(params) ? params : [],
            rowMode: null,
        };
    }

    if (!queryOrConfig || typeof queryOrConfig.text !== "string") {
        throw new TypeError("Flux.postgres NodePgPool.query expects a SQL string or a query config with text");
    }

    return {
        text: queryOrConfig.text,
        params: Array.isArray(params)
            ? params
            : Array.isArray(queryOrConfig.values)
                ? queryOrConfig.values
                : [],
        rowMode: queryOrConfig.rowMode === "array" ? "array" : null,
    };
}
function __flux_node_pg_field_list(rows) {
    const first = Array.isArray(rows) ? rows[0] : null;
    if (!first || typeof first !== "object" || Array.isArray(first)) return [];
    return Object.keys(first).map((name) => ({
        name,
        dataTypeID: 0,
        format: "text",
    }));
}
function __flux_node_pg_rows(rows, rowMode) {
    if (!Array.isArray(rows)) return [];
    if (rowMode !== "array") return rows;
    return rows.map((row) => {
        if (!row || typeof row !== "object" || Array.isArray(row)) return [];
        return Object.values(row);
    });
}
class FluxNodePgClient {
    constructor(pool, sessionId) {
        this.pool = pool;
        this.sessionId = sessionId;
        this.released = false;
    }

    async query(queryOrConfig, params = undefined) {
        if (this.released) {
            throw new Error("Flux.postgres NodePgClient has already been released");
        }
        const normalized = __flux_normalize_node_pg_query(queryOrConfig, params);
        const response = Deno.core.ops.op_flux_postgres_session_query({
            execution_id: __flux_eid(),
            session_id: this.sessionId,
            sql: normalized.text,
            params: normalized.params,
        });
        const fields = __flux_node_pg_field_list(response.rows);
        const rows = __flux_node_pg_rows(response.rows, normalized.rowMode);
        return {
            command: response.command ?? null,
            rowCount: rows.length,
            rows,
            fields,
        };
    }

    async release() {
        if (this.released) return undefined;
        this.released = true;
        Deno.core.ops.op_flux_postgres_close_session({
            execution_id: __flux_eid(),
            session_id: this.sessionId,
        });
        this.pool.__clients.delete(this);
        return undefined;
    }
}
class FluxNodePgPool {
    constructor(options = {}) {
        this.connectionString = String(options.connectionString ?? "");
        this.tls = !!options.tls;
        this.caCertPem = options.caCertPem == null ? null : String(options.caCertPem);
        this.__clients = new Set();
        this.__ended = false;
    }

    async query(queryOrConfig, params = undefined) {
        if (this.__ended) {
            throw new Error("Flux.postgres NodePgPool has already been ended");
        }
        const normalized = __flux_normalize_node_pg_query(queryOrConfig, params);
        const response = globalThis.Flux.postgres.query({
            connectionString: this.connectionString,
            sql: normalized.text,
            params: normalized.params,
            tls: this.tls,
            caCertPem: this.caCertPem,
        });
        const fields = __flux_node_pg_field_list(response.rows);
        const rows = __flux_node_pg_rows(response.rows, normalized.rowMode);
        return {
            command: response.command ?? null,
            rowCount: rows.length,
            rows,
            fields,
        };
    }

    async connect() {
        if (this.__ended) {
            throw new Error("Flux.postgres NodePgPool has already been ended");
        }
        const response = Deno.core.ops.op_flux_postgres_connect({
            execution_id: __flux_eid(),
            connection_string: this.connectionString,
            tls: this.tls,
            ca_cert_pem: this.caCertPem,
        });
        const client = new FluxNodePgClient(this, String(response.sessionId ?? ""));
        this.__clients.add(client);
        return client;
    }

    async end() {
        this.__ended = true;
        const clients = Array.from(this.__clients);
        await Promise.all(clients.map((client) => client.release()));
        return undefined;
    }
}
globalThis.Flux.postgres.NodePgClient = FluxNodePgClient;
globalThis.Flux.postgres.NodePgPool = FluxNodePgPool;
globalThis.Flux.postgres.createNodePgPool = function(options = {}) {
    return new FluxNodePgPool(options);
};
globalThis.Flux.postgres.query = function(options = {}) {
    const response = Deno.core.ops.op_flux_postgres_query({
        execution_id: __flux_eid(),
        connection_string: String(options.connectionString ?? ""),
        sql: String(options.sql ?? ""),
        params: Array.isArray(options.params) ? options.params : [],
        tls: !!options.tls,
        ca_cert_pem: options.caCertPem == null ? null : String(options.caCertPem),
    });

    return {
        rows: Array.isArray(response.rows) ? response.rows : [],
        command: response.command ?? null,
        replay: !!response.replay,
    };
};
globalThis.Flux.postgres.simpleQuery = function(options = {}) {
    const response = Deno.core.ops.op_flux_postgres_simple_query({
        execution_id: __flux_eid(),
        connection_string: String(options.connectionString ?? ""),
        sql: String(options.sql ?? ""),
        tls: !!options.tls,
        ca_cert_pem: options.caCertPem == null ? null : String(options.caCertPem),
    });

    return {
        rows: Array.isArray(response.rows) ? response.rows : [],
        command: response.command ?? null,
        replay: !!response.replay,
    };
};
globalThis.Flux.net.tcpExchange = function(options = {}) {
    const encodedText = Array.from(new TextEncoder().encode(options.text ?? ""));
    const inputBytes = ArrayBuffer.isView(options.data)
        ? Array.from(new Uint8Array(options.data.buffer, options.data.byteOffset, options.data.byteLength))
        : Array.isArray(options.data)
            ? options.data
            : encodedText;

    const response = Deno.core.ops.op_flux_tcp_exchange({
        execution_id: __flux_eid(),
        host: String(options.host ?? ""),
        port: Number(options.port ?? 0),
        write_bytes: inputBytes,
        read_mode: options.readMode ?? "until_close",
        read_bytes: options.readBytes ?? null,
        connect_timeout_ms: options.connectTimeoutMs ?? null,
        read_timeout_ms: options.readTimeoutMs ?? null,
        tls: !!options.tls,
        server_name: options.serverName ?? null,
        ca_cert_pem: options.caCertPem ?? null,
    });

    const bytes = Uint8Array.from(response.bytes ?? []);
    return {
        bytes,
        replay: !!response.replay,
        text: new TextDecoder().decode(bytes),
    };
};

// ── Deno.serve (server mode) ─────────────────────────────────────────────────
globalThis.__flux_net_handler = null;
globalThis.__flux_net_server = null;

function __flux_close_server(serverState, reason = undefined) {
    if (!serverState || serverState.closed) return;
    serverState.closed = true;
    serverState.reason = reason;
    if (typeof serverState.resolveFinished === "function") {
        serverState.resolveFinished();
        serverState.resolveFinished = null;
    }
}

Deno.serve = function(optionsOrHandler, maybeHandler = undefined) {
    let options = null;
    let handler;

    if (typeof optionsOrHandler === "function") {
        handler = optionsOrHandler;
    } else if (optionsOrHandler && typeof maybeHandler === "function") {
        options = optionsOrHandler;
        handler = maybeHandler;
    } else if (optionsOrHandler && typeof optionsOrHandler.fetch === "function") {
        options = optionsOrHandler;
        handler = optionsOrHandler.fetch.bind(optionsOrHandler);
    } else if (optionsOrHandler && typeof optionsOrHandler.handler === "function") {
        options = optionsOrHandler;
        handler = optionsOrHandler.handler;
    }

  if (!handler) throw new TypeError("Deno.serve: expected a handler function or { fetch } object");

    const signal = options && options.signal !== undefined ? options.signal : null;
    let resolveFinished = null;
    const finished = new Promise((resolve) => {
        resolveFinished = resolve;
    });

    const server = {
        finished,
        ref() { return server; },
        unref() { return server; },
        shutdown(reason = undefined) {
            __flux_close_server(serverState, reason);
            return finished;
        },
    };

    const serverState = {
        handler,
        closed: false,
        reason: undefined,
        resolveFinished,
        finished,
        server,
    };

    globalThis.__flux_net_server = serverState;
    globalThis.__flux_net_handler = handler;

    if (signal && signal.aborted) {
        __flux_close_server(serverState, signal.reason);
        return server;
    }

  Deno.core.ops.op_net_listen(__flux_eid(), 0);

    if (options && typeof options.onListen === "function") {
        if (Object.prototype.hasOwnProperty.call(options, "path")) {
            options.onListen({ path: String(options.path) });
        } else {
            options.onListen({
                hostname: String(options.hostname ?? "0.0.0.0"),
                port: Number(options.port ?? 8000),
            });
        }
    }

    if (signal && typeof signal.addEventListener === "function") {
        signal.addEventListener("abort", () => {
            __flux_close_server(serverState, signal.reason);
        });
    }

    return server;
};

// Called by Rust (via execute_script) for each incoming HTTP request.
globalThis.__flux_dispatch_request = async function(reqId, method, url, headersJson, body) {
  const __eid = globalThis.__FLUX_EXECUTION_ID__;
    const serverState = globalThis.__flux_net_server;
    const handler = serverState && !serverState.closed
        ? serverState.handler
        : globalThis.__flux_net_handler;
    if (serverState && serverState.closed) {
        const message = serverState.reason == null ? "Server closed" : String(serverState.reason);
        Deno.core.ops.op_net_respond(__eid, reqId, 503, "[]", message);
        return;
    }
  if (!handler) {
    Deno.core.ops.op_net_respond(__eid, reqId, 500, "[]", "No Deno.serve handler registered");
    return;
  }

  let headersInit;
  try {
    headersInit = JSON.parse(headersJson);
  } catch {
    headersInit = [];
  }

    const requestHeaders = new Headers(headersInit);
    let requestUrl = String(url);
    const hasScheme = /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(requestUrl);
    if (!hasScheme) {
        const host = requestHeaders.get("host") || "127.0.0.1";
        if (requestUrl.startsWith("/")) {
            requestUrl = `http://${host}${requestUrl}`;
        } else {
            requestUrl = `http://${requestUrl}`;
        }
    }

    const request = new Request(requestUrl, {
    method,
        headers: requestHeaders,
    body: (method === "GET" || method === "HEAD") ? undefined : (body || undefined),
  });

  let response;
  try {
    response = await handler(request);
  } catch (err) {
    const msg = String(err && err.stack ? err.stack : err);
    Deno.core.ops.op_net_respond(__eid, reqId, 500, "[]", msg);
    return;
  }

    if (!(response instanceof Response)) {
        response = new Response(response == null ? "" : response);
    }

  let responseBody;
  try { responseBody = await response.text(); } catch { responseBody = ""; }

  const responseHeaders = JSON.stringify([...response.headers.entries()]);
  Deno.core.ops.op_net_respond(__eid, reqId, response.status ?? 200, responseHeaders, responseBody);
};
"#
}

fn prepare_user_code(code: &str) -> String {
    let transformed = rewrite_export_default(code);

    // In server mode (Deno.serve was called) __flux_net_handler is set instead
    // of __flux_user_handler — skip the export guard in that case.
    format!(
        "{}\n\
         if (typeof globalThis.__flux_net_handler !== 'function' && \
             typeof globalThis.__flux_user_handler !== 'function') {{\n\
           throw new Error('entry module must export default function or call Deno.serve()');\n\
         }}",
        transformed
    )
}

/// Like `prepare_user_code` but without the mandatory-export guard, so plain
/// top-level scripts (no `export default`) can run without throwing.
/// Used exclusively by the `flux run` / `--script-mode` path.
fn prepare_run_code(code: &str) -> String {
    rewrite_export_default(code)
}

/// Rewrite `export default [async] function` / `export default <expr>` into
/// `globalThis.__flux_user_handler = …` so the Rust host can invoke the
/// handler without ES module machinery.
fn rewrite_export_default(code: &str) -> String {
    if code.contains("export default async function") {
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
    }
}
