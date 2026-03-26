use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::net::{IpAddr, Shutdown, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Once;
use std::sync::mpsc;
use std::sync::{OnceLock, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::{
    STANDARD as BASE64_STANDARD, URL_SAFE as BASE64_URL_SAFE,
    URL_SAFE_NO_PAD as BASE64_URL_SAFE_NO_PAD,
};
use bytes::BytesMut;
use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime, SecondsFormat, Utc};
use deno_ast::{
    EmitOptions, MediaType, ParseParams, SourceMapOption, TranspileModuleOptions, TranspileOptions,
};
use deno_core::{
    JsRuntime, ModuleLoadOptions, ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader,
    ModuleSource, ModuleSourceCode, ModuleSpecifier, ModuleType, OpState, ResolutionKind,
    RuntimeOptions, op2, resolve_import, resolve_path,
};
use deno_error::JsErrorBox;
use postgres::config::SslMode as PostgresSslMode;
use postgres::types::{FromSql, IsNull, ToSql, Type as PostgresType};
use postgres::{Client as PostgresClient, Config as PostgresConfig, NoTls, SimpleQueryMessage};
use postgres_rustls::MakeTlsConnector as PostgresMakeTlsConnector;
use rand::Rng;
use rsa::pkcs1v15::{Signature as RsaPkcs1v15Signature, VerifyingKey as RsaPkcs1v15VerifyingKey};
use rsa::signature::Verifier;
use rsa::{BigUint as RsaBigUint, RsaPublicKey};
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use shared::project::{ArtifactMediaType, ArtifactModule, ArtifactSourceKind, FluxBuildArtifact};
use tokio_rustls::TlsConnector;
use ureq::OrAnyStatus;
use url::Url;
use uuid::Uuid;

use crate::artifact::{sha256_hex, RuntimeArtifact};
use crate::isolate_pool::{ExecutionContext, ExecutionResult};

const FLUX_PG_SPECIFIER: &str = "flux:pg";
const FLUX_REDIS_SPECIFIER: &str = "flux:redis";
const FLUX_HTTP_SPECIFIER: &str = "flux:http";
const MODULE_FILE_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx", "mjs", "json"];

#[derive(Debug, Default, Deserialize)]
struct RuntimePackageManifest {
    main: Option<String>,
    module: Option<String>,
}

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
const DEFAULT_HTTP_CACHE_MAX_ENTRIES: usize = 1_000;
const DEFAULT_HTTP_CACHE_MAX_BYTES: usize = 50 * 1024 * 1024;
static CRASH_AFTER_POSTGRES_COMMIT_BEFORE_CHECKPOINT: AtomicBool = AtomicBool::new(true);

fn postgres_sql_is_write(sql: &str) -> bool {
    matches!(
        sql.split_whitespace()
            .next()
            .map(|value| value.to_ascii_uppercase())
            .as_deref(),
        Some("INSERT" | "UPDATE" | "DELETE" | "MERGE")
    )
}

fn maybe_crash_after_postgres_commit_before_checkpoint(sql: &str) {
    if !postgres_sql_is_write(sql) {
        return;
    }

    let enabled = std::env::var("FLUX_CRASH_AFTER_POSTGRES_COMMIT_BEFORE_CHECKPOINT")
        .map(|value| value == "1")
        .unwrap_or(false);

    if enabled
        && CRASH_AFTER_POSTGRES_COMMIT_BEFORE_CHECKPOINT.swap(false, Ordering::SeqCst)
    {
        tracing::error!(
            "crashing after postgres commit before checkpoint capture by request"
        );
        std::process::exit(1);
    }
}

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

    let allow_loopback = std::env::var("FLUXBASE_ALLOW_LOOPBACK_FETCH")
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

    if allow_loopback {
        return Ok(());
    }

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
                    format!(
                        "{blocked_label} blocked: private/loopback IP addresses are not allowed"
                    ),
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
            || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
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

#[derive(Debug, Deserialize)]
struct Rs256VerifyRequest {
    jwk: serde_json::Value,
    data_base64: String,
    signature_base64: String,
}

#[derive(Debug, Clone, Default)]
struct HttpCacheControl {
    max_age: Option<u64>,
    s_maxage: Option<u64>,
    no_cache: bool,
    no_store: bool,
    private: bool,
    public: bool,
}

#[derive(Debug, Clone)]
struct HttpCacheEntry {
    vary_headers: BTreeMap<String, String>,
    response: serde_json::Value,
    cached_at: Instant,
    expires_at: Instant,
    size_bytes: usize,
    last_accessed_tick: u64,
}

#[derive(Debug, Clone, Copy)]
struct HttpCacheLimits {
    max_entries: usize,
    max_bytes: usize,
}

#[derive(Debug, Default)]
struct HttpResponseCache {
    entries_by_key: HashMap<String, Vec<HttpCacheEntry>>,
    total_entries: usize,
    total_bytes: usize,
    access_tick: u64,
}

impl HttpResponseCache {
    fn clear(&mut self) {
        self.entries_by_key.clear();
        self.total_entries = 0;
        self.total_bytes = 0;
        self.access_tick = 0;
    }

    fn next_access_tick(&mut self) -> u64 {
        self.access_tick = self.access_tick.saturating_add(1);
        self.access_tick
    }

    fn remove_expired_entries(&mut self, now: Instant) {
        let mut empty_keys = Vec::new();
        for (key, entries) in self.entries_by_key.iter_mut() {
            let mut removed_entries = 0usize;
            let mut removed_bytes = 0usize;
            entries.retain(|entry| {
                let keep = entry.expires_at > now;
                if !keep {
                    removed_entries += 1;
                    removed_bytes += entry.size_bytes;
                }
                keep
            });

            self.total_entries = self.total_entries.saturating_sub(removed_entries);
            self.total_bytes = self.total_bytes.saturating_sub(removed_bytes);

            if entries.is_empty() {
                empty_keys.push(key.clone());
            }
        }

        for key in empty_keys {
            self.entries_by_key.remove(&key);
        }
    }

    fn apply_limits(&mut self, limits: HttpCacheLimits, now: Instant) {
        self.remove_expired_entries(now);

        if limits.max_entries == 0 || limits.max_bytes == 0 {
            self.clear();
            return;
        }

        while self.total_entries > limits.max_entries || self.total_bytes > limits.max_bytes {
            let mut candidate: Option<(String, BTreeMap<String, String>, u64)> = None;

            for (key, entries) in &self.entries_by_key {
                for entry in entries {
                    let replace = candidate
                        .as_ref()
                        .map(|(_, _, tick)| entry.last_accessed_tick < *tick)
                        .unwrap_or(true);
                    if replace {
                        candidate = Some((
                            key.clone(),
                            entry.vary_headers.clone(),
                            entry.last_accessed_tick,
                        ));
                    }
                }
            }

            let Some((key, vary_headers, _)) = candidate else {
                break;
            };

            self.remove_entry(&key, &vary_headers);
        }
    }

    fn remove_entry(&mut self, key: &str, vary_headers: &BTreeMap<String, String>) {
        let mut should_remove_key = false;
        if let Some(entries) = self.entries_by_key.get_mut(key) {
            if let Some(index) = entries
                .iter()
                .position(|entry| &entry.vary_headers == vary_headers)
            {
                let removed = entries.remove(index);
                self.total_entries = self.total_entries.saturating_sub(1);
                self.total_bytes = self.total_bytes.saturating_sub(removed.size_bytes);
            }
            should_remove_key = entries.is_empty();
        }

        if should_remove_key {
            self.entries_by_key.remove(key);
        }
    }

    fn lookup(
        &mut self,
        key: &str,
        request_headers: &BTreeMap<String, String>,
        now: Instant,
        limits: HttpCacheLimits,
    ) -> Option<serde_json::Value> {
        self.apply_limits(limits, now);

        let found_index = {
            let entries = self.entries_by_key.get(key)?;
            entries.iter().position(|entry| {
                entry.vary_headers.iter().all(|(name, value)| {
                    request_headers.get(name).cloned().unwrap_or_default() == *value
                })
            })
        }?;

        let access_tick = self.next_access_tick();
        let response = {
            let entries = self.entries_by_key.get_mut(key)?;
            let entry = entries.get_mut(found_index)?;
            entry.last_accessed_tick = access_tick;
            cache_hit_response(entry, now)
        };

        Some(response)
    }

    fn store(
        &mut self,
        key: String,
        limits: HttpCacheLimits,
        now: Instant,
        mut entry: HttpCacheEntry,
    ) {
        self.apply_limits(limits, now);

        if limits.max_entries == 0 || limits.max_bytes == 0 {
            return;
        }

        if entry.size_bytes > limits.max_bytes {
            return;
        }

        self.remove_entry(&key, &entry.vary_headers);

        entry.last_accessed_tick = self.next_access_tick();
        let entries = self.entries_by_key.entry(key).or_default();
        entries.push(entry.clone());
        self.total_entries += 1;
        self.total_bytes += entry.size_bytes;
        self.apply_limits(limits, now);
    }
}

fn http_response_cache_limits() -> HttpCacheLimits {
    HttpCacheLimits {
        max_entries: read_cache_limit_env(
            "FLUXBASE_HTTP_CACHE_MAX_ENTRIES",
            DEFAULT_HTTP_CACHE_MAX_ENTRIES,
        ),
        max_bytes: read_cache_limit_env(
            "FLUXBASE_HTTP_CACHE_MAX_BYTES",
            DEFAULT_HTTP_CACHE_MAX_BYTES,
        ),
    }
}

fn read_cache_limit_env(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(default)
}

fn estimate_cache_entry_size(
    key: &str,
    vary_headers: &BTreeMap<String, String>,
    response: &serde_json::Value,
) -> usize {
    let vary_size = vary_headers
        .iter()
        .map(|(header, value)| header.len() + value.len())
        .sum::<usize>();
    let response_size = serde_json::to_vec(response)
        .map(|bytes| bytes.len())
        .unwrap_or(0);

    key.len() + vary_size + response_size
}

fn http_response_cache() -> &'static RwLock<HttpResponseCache> {
    static CACHE: OnceLock<RwLock<HttpResponseCache>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HttpResponseCache::default()))
}

#[doc(hidden)]
pub fn reset_http_response_cache_for_tests() {
    if let Ok(mut cache) = http_response_cache().write() {
        cache.clear();
    }
}

fn parse_cache_control(value: Option<&str>) -> HttpCacheControl {
    let mut directives = HttpCacheControl::default();

    let Some(value) = value else {
        return directives;
    };

    for part in value.split(',') {
        let directive = part.trim();
        if directive.is_empty() {
            continue;
        }

        let mut pieces = directive.splitn(2, '=');
        let key = pieces
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let raw_value = pieces.next().map(|value| value.trim().trim_matches('"'));

        match key.as_str() {
            "max-age" => directives.max_age = raw_value.and_then(|value| value.parse::<u64>().ok()),
            "s-maxage" => {
                directives.s_maxage = raw_value.and_then(|value| value.parse::<u64>().ok())
            }
            "no-cache" => directives.no_cache = true,
            "no-store" => directives.no_store = true,
            "private" => directives.private = true,
            "public" => directives.public = true,
            _ => {}
        }
    }

    directives
}

fn parse_http_date(value: Option<&str>) -> Option<DateTime<Utc>> {
    let value = value?.trim();
    DateTime::parse_from_rfc2822(value)
        .ok()
        .map(|parsed| parsed.with_timezone(&Utc))
}

fn cache_lookup_allowed(headers: &BTreeMap<String, String>) -> bool {
    let cache_control = parse_cache_control(headers.get("cache-control").map(String::as_str));
    if cache_control.no_cache || cache_control.no_store || cache_control.max_age == Some(0) {
        return false;
    }

    !matches!(headers.get("pragma"), Some(value) if value.eq_ignore_ascii_case("no-cache"))
        && !headers.contains_key("if-none-match")
        && !headers.contains_key("if-modified-since")
        && !headers.contains_key("if-match")
        && !headers.contains_key("if-unmodified-since")
}

fn cache_store_allowed(
    method: &str,
    status: u16,
    request_headers: &BTreeMap<String, String>,
    response_headers: &BTreeMap<String, String>,
) -> Option<Duration> {
    if !matches!(method, "GET" | "HEAD") {
        return None;
    }

    if !(200..300).contains(&status) {
        return None;
    }

    let request_cache_control =
        parse_cache_control(request_headers.get("cache-control").map(String::as_str));
    if request_cache_control.no_store {
        return None;
    }

    let response_cache_control =
        parse_cache_control(response_headers.get("cache-control").map(String::as_str));
    if response_cache_control.no_store || response_cache_control.private {
        return None;
    }

    let ttl_secs = response_cache_control
        .s_maxage
        .or(response_cache_control.max_age)
        .or_else(|| {
            let expires_at = parse_http_date(response_headers.get("expires").map(String::as_str))?;
            let now = parse_http_date(response_headers.get("date").map(String::as_str))
                .unwrap_or_else(Utc::now);
            let delta = expires_at.signed_duration_since(now);
            if delta.num_seconds() <= 0 {
                None
            } else {
                Some(delta.num_seconds() as u64)
            }
        })?;

    if ttl_secs == 0 {
        return None;
    }

    let vary = response_headers
        .get("vary")
        .map(String::as_str)
        .unwrap_or("");
    if vary.split(',').any(|name| name.trim() == "*") {
        return None;
    }

    Some(Duration::from_secs(ttl_secs))
}

fn response_vary_headers(
    response_headers: &BTreeMap<String, String>,
    request_headers: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    response_headers
        .get("vary")
        .map(String::as_str)
        .unwrap_or("")
        .split(',')
        .filter_map(|name| {
            let key = name.trim().to_ascii_lowercase();
            if key.is_empty() || key == "*" {
                return None;
            }
            Some((
                key.clone(),
                request_headers.get(&key).cloned().unwrap_or_default(),
            ))
        })
        .collect()
}

fn cache_key(method: &str, url: &str) -> String {
    format!("{method}:{url}")
}

fn cache_hit_response(entry: &HttpCacheEntry, now: Instant) -> serde_json::Value {
    let mut response = entry.response.clone();
    let age_ms = now
        .checked_duration_since(entry.cached_at)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    if let Some(object) = response.as_object_mut() {
        object.insert(
            "cache".to_string(),
            serde_json::json!({
                "hit": true,
                "source": "memory",
                "age_ms": age_ms,
            }),
        );
    }
    response
}

fn cached_http_response(
    method: &str,
    url: &str,
    request_headers: &BTreeMap<String, String>,
) -> Option<serde_json::Value> {
    if !cache_lookup_allowed(request_headers) {
        return None;
    }

    let key = cache_key(method, url);
    let now = Instant::now();
    let limits = http_response_cache_limits();
    let cache = http_response_cache();
    let mut cache = cache.write().ok()?;
    cache.lookup(&key, request_headers, now, limits)
}

fn store_http_response_cache(
    method: &str,
    url: &str,
    request_headers: &BTreeMap<String, String>,
    response_headers: &BTreeMap<String, String>,
    response: &serde_json::Value,
    status: u16,
) {
    let Some(ttl) = cache_store_allowed(method, status, request_headers, response_headers) else {
        return;
    };

    let entry = HttpCacheEntry {
        vary_headers: response_vary_headers(response_headers, request_headers),
        response: response.clone(),
        cached_at: Instant::now(),
        expires_at: Instant::now() + ttl,
        size_bytes: 0,
        last_accessed_tick: 0,
    };

    let key = cache_key(method, url);
    if let Ok(mut cache) = http_response_cache().write() {
        let now = Instant::now();
        let limits = http_response_cache_limits();
        let mut entry = entry;
        entry.size_bytes = estimate_cache_entry_size(&key, &entry.vary_headers, &entry.response);
        cache.store(key, limits, now, entry);
    }
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

#[derive(Debug, Clone, Deserialize)]
struct RedisCommandRequestPayload {
    execution_id: String,
    connection_string: String,
    command: String,
    #[serde(default)]
    args: Vec<serde_json::Value>,
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

#[derive(Debug, Clone)]
struct RedisConnectionTarget {
    url: String,
    host: String,
    port: u16,
    db: i64,
    username: Option<String>,
    password: Option<String>,
}

enum PostgresSessionCommand {
    Query {
        sql: String,
        params: Vec<serde_json::Value>,
        reply: mpsc::Sender<std::result::Result<PostgresSimpleQueryResponse, PostgresDriverError>>,
    },
    Close,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PostgresDriverError {
    message: String,
    code: Option<String>,
    detail: Option<String>,
    constraint: Option<String>,
    schema: Option<String>,
    table: Option<String>,
    column: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RedisDriverError {
    message: String,
}

impl RedisDriverError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "message": self.message,
        })
    }
}

impl PostgresDriverError {
    fn from_postgres_error(err: postgres::Error, default_prefix: &str) -> Self {
        if let Some(db_error) = err.as_db_error() {
            Self {
                message: db_error.message().to_string(),
                code: Some(db_error.code().code().to_string()),
                detail: db_error.detail().map(str::to_string),
                constraint: db_error.constraint().map(str::to_string),
                schema: db_error.schema().map(str::to_string),
                table: db_error.table().map(str::to_string),
                column: db_error.column().map(str::to_string),
            }
        } else {
            Self {
                message: format!("{default_prefix}: {err}"),
                code: None,
                detail: None,
                constraint: None,
                schema: None,
                table: None,
                column: None,
            }
        }
    }

    fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "message": self.message,
            "code": self.code,
            "detail": self.detail,
            "constraint": self.constraint,
            "schema": self.schema,
            "table": self.table,
            "column": self.column,
        })
    }
}

#[derive(Debug)]
enum PostgresQueryOutcome {
    Success(PostgresSimpleQueryResponse),
    Error(PostgresDriverError),
}

#[derive(Debug)]
enum RedisCommandOutcome {
    Success(serde_json::Value),
    Error(RedisDriverError),
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
                Err(JsErrorBox::generic(
                    "postgres connect failed: session worker exited before initialization",
                ))
            }
        }
    }

    fn query(
        &self,
        sql: &str,
        params: &[serde_json::Value],
    ) -> Result<PostgresQueryOutcome, JsErrorBox> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.sender
            .send(PostgresSessionCommand::Query {
                sql: sql.to_string(),
                params: params.to_vec(),
                reply: reply_tx,
            })
            .map_err(|_| JsErrorBox::generic("postgres session is closed"))?;

        match reply_rx.recv() {
            Ok(Ok(response)) => Ok(PostgresQueryOutcome::Success(response)),
            Ok(Err(err)) => Ok(PostgresQueryOutcome::Error(err)),
            Err(_) => Err(JsErrorBox::generic(
                "postgres session worker exited unexpectedly",
            )),
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
    pub error: Option<String>,
    pub logs: Vec<LogEntry>,
    pub has_live_io: bool,
    /// Set when replay stops at an IO boundary with no recorded checkpoint.
    /// Value is the boundary name (e.g. "postgres", "http", "tcp", "redis").
    pub boundary_stop: Option<String>,
}

#[derive(Debug, Clone)]
pub struct JsExecutionOutput {
    pub output: serde_json::Value,
    pub checkpoints: Vec<FetchCheckpoint>,
    pub error: Option<String>,
    pub logs: Vec<LogEntry>,
    pub has_live_io: bool,
    /// Set when replay stops at an IO boundary with no recorded checkpoint.
    pub boundary_stop: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BootExecutionResult {
    pub result: ExecutionResult,
    pub is_server_mode: bool,
    pub has_handler: bool,
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
    /// True if any live IO was performed during a Replay execution.
    pub has_live_io: bool,
    /// Set when replay stops at an IO boundary with no recorded checkpoint.
    /// Value is the boundary name (e.g. "postgres", "http", "tcp", "redis").
    pub boundary_stop: Option<String>,
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

deno_core::extension!(
    deno_node,
    esm = [
        dir "src",
        "internal/crypto/constants.ts"
    ],
);

deno_core::extension!(
    flux_runtime_ext,
    deps = [deno_webidl, deno_web, deno_node, deno_crypto],
    ops = [
        op_begin_execution,
        op_end_execution,
        op_crypto_verify_rs256,
        op_flux_fetch,
        op_flux_tcp_exchange,
        op_flux_redis_command,
        op_flux_postgres_connect,
        op_flux_postgres_close_session,
        op_flux_postgres_simple_query,
        op_flux_postgres_query,
        op_flux_postgres_session_query,
        op_flux_env_get,
        op_flux_env_list,
        op_flux_now,
        op_flux_now_high_res,
        op_flux_parse_url,
        op_console,
        op_timer_delay,
        op_random,
        op_random_uuid,
        op_random_bytes,
        op_flux_crypto_replay,
        op_flux_crypto_record,
        op_net_listen,
        op_net_respond,
    ],
    esm_entry_point = "ext:flux_runtime_ext/bootstrap_crypto.js",
    esm = [
        dir "src",
        "bootstrap_flux.js",
        "bootstrap_crypto.js"
    ],
);

fn flux_extensions() -> Vec<deno_core::Extension> {
    vec![
        deno_webidl::deno_webidl::init(),
        deno_web::deno_web::init(
            std::sync::Arc::new(deno_web::BlobStore::default()),
            None,
            deno_web::InMemoryBroadcastChannel::default(),
        ),
        deno_crypto::deno_crypto::init(None),
        deno_node::init(),
        flux_runtime_ext::init(),
    ]
}

fn decode_base64_field(value: &str, field: &str) -> Result<Vec<u8>, JsErrorBox> {
    BASE64_STANDARD
        .decode(value)
        .map_err(|error| JsErrorBox::type_error(format!("invalid base64 for {field}: {error}")))
}

fn decode_base64url_field(value: &str, field: &str) -> Result<Vec<u8>, JsErrorBox> {
    BASE64_URL_SAFE_NO_PAD
        .decode(value)
        .or_else(|_| BASE64_URL_SAFE.decode(value))
        .map_err(|error| JsErrorBox::type_error(format!("invalid base64url for {field}: {error}")))
}

fn jwk_string_field<'a>(
    jwk: &'a serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Result<&'a str, JsErrorBox> {
    jwk.get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| JsErrorBox::type_error(format!("JWK is missing string field '{field}'")))
}

#[op2]
fn op_crypto_verify_rs256(#[serde] request: serde_json::Value) -> Result<bool, JsErrorBox> {
    let request: Rs256VerifyRequest = serde_json::from_value(request).map_err(|error| {
        JsErrorBox::type_error(format!("invalid rs256 verify request: {error}"))
    })?;

    let jwk = request
        .jwk
        .as_object()
        .ok_or_else(|| JsErrorBox::type_error("JWK must be an object"))?;

    let kty = jwk_string_field(jwk, "kty")?;
    if kty != "RSA" {
        return Err(JsErrorBox::type_error("only RSA JWKs are supported"));
    }

    if let Some(alg) = jwk.get("alg").and_then(serde_json::Value::as_str) {
        if alg != "RS256" {
            return Err(JsErrorBox::type_error("only RS256 JWKs are supported"));
        }
    }

    if let Some(use_field) = jwk.get("use").and_then(serde_json::Value::as_str) {
        if use_field != "sig" {
            return Err(JsErrorBox::type_error("RSA JWK must be for signature use"));
        }
    }

    let modulus = decode_base64url_field(jwk_string_field(jwk, "n")?, "jwk.n")?;
    let exponent = decode_base64url_field(jwk_string_field(jwk, "e")?, "jwk.e")?;
    let data = decode_base64_field(&request.data_base64, "data_base64")?;
    let signature = decode_base64_field(&request.signature_base64, "signature_base64")?;

    let public_key = RsaPublicKey::new(
        RsaBigUint::from_bytes_be(&modulus),
        RsaBigUint::from_bytes_be(&exponent),
    )
    .map_err(|error| JsErrorBox::type_error(format!("invalid RSA public key: {error}")))?;
    let verifying_key = RsaPkcs1v15VerifyingKey::<Sha256>::new(public_key);
    let signature = RsaPkcs1v15Signature::try_from(signature.as_slice())
        .map_err(|error| JsErrorBox::type_error(format!("invalid RSA signature: {error}")))?;

    Ok(verifying_key.verify(&data, &signature).is_ok())
}

#[op2]
#[string]
fn op_flux_env_get(#[string] key: String) -> Option<String> {
    std::env::var(key).ok()
}

#[op2]
#[serde]
fn op_flux_env_list() -> std::collections::HashMap<String, String> {
    std::env::vars().collect()
}

/// Called by JS at the start of every execution to register a state slot.
/// `recorded_random_json` and `recorded_uuids_json` are JSON-encoded arrays for
/// replay mode; pass `"[]"` for live executions.
#[op2]
fn op_begin_execution(
    state: &mut OpState,
    #[string] execution_id: String,
    #[string] request_id: String,
    #[string] code_version: String,
    #[string] project_id: Option<String>,
    is_replay: bool,
    #[string] recorded_random_json: String,
    #[string] recorded_uuids_json: String,
    #[string] recorded_now_ms_json: String,
) {
    let recorded_random: Vec<f64> = serde_json::from_str(&recorded_random_json).unwrap_or_default();
    let recorded_uuids: Vec<String> =
        serde_json::from_str(&recorded_uuids_json).unwrap_or_default();
    let recorded_now_ms: Option<u64> = serde_json::from_str(&recorded_now_ms_json).unwrap_or(None);

    let exec_state = RuntimeExecutionState {
        context: ExecutionContext {
            execution_id: execution_id.clone(),
            request_id,
            project_id,
            code_version,
            mode: if is_replay {
                ExecutionMode::Replay
            } else {
                ExecutionMode::Live
            },
            verbose: false,
        },
        call_index: 0,
        checkpoints: Vec::new(),
        recorded: HashMap::new(),
        recorded_now_ms,
        logs: Vec::new(),
        has_live_io: false,
        boundary_stop: None,
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
    let slot = state.borrow_mut::<RuntimeStateMap>().remove(&execution_id);

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
                    "Error",
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
                        boundary: checkpoint.boundary.clone(),
                        url: checkpoint.url.clone(),
                        method: checkpoint.method.clone(),
                        request: checkpoint.request.clone(),
                        response: response.clone(),
                        duration_ms: checkpoint.duration_ms,
                    });
                }
            }
            tracing::debug!(%request_id, %call_index, "replay: returned recorded response");
            print_checkpoint_replay(&FetchCheckpoint {
                call_index,
                boundary: checkpoint.boundary.clone(),
                url: checkpoint.url.clone(),
                method: checkpoint.method.clone(),
                request: checkpoint.request.clone(),
                response: response.clone(),
                duration_ms: checkpoint.duration_ms,
            });
            return Ok(response)
        }
        {
            let map = state.borrow_mut::<RuntimeStateMap>();
            if let Some(execution) = map.get_mut(&execution_id) {
                execution.boundary_stop = Some("http".to_string());
            }
        }
        tracing::debug!(%request_id, %call_index, url = %original_url, method = %method, "replay: stopping at http boundary (no recorded checkpoint)");
        return Err(JsErrorBox::generic(format!("__FLUX_BOUNDARY_STOP:http:{call_index}")));
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
                "Error",
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

    validate_outbound_host(&host, port, "tcp connect", "FLUXBASE_ALLOW_LOOPBACK_TCP")?;

    if matches!(mode, ExecutionMode::Replay) {
        if let Some(checkpoint) = recorded_checkpoint {
            let response = checkpoint.response.clone();
            let bytes = decode_checkpoint_bytes(&response, "response_base64")?;
            {
                let map = state.borrow_mut::<RuntimeStateMap>();
                if let Some(execution) = map.get_mut(&execution_id) {
                    execution.checkpoints.push(FetchCheckpoint {
                        call_index,
                        boundary: checkpoint.boundary.clone(),
                        url: checkpoint.url.clone(),
                        method: checkpoint.method.clone(),
                        request: checkpoint.request.clone(),
                        response: response.clone(),
                        duration_ms: checkpoint.duration_ms,
                    });
                }
            }
            tracing::debug!(%request_id, %call_index, host = %host, port = %port, "replay: returned recorded tcp exchange");
            print_checkpoint_replay(&FetchCheckpoint {
                call_index,
                boundary: checkpoint.boundary.clone(),
                url: checkpoint.url.clone(),
                method: checkpoint.method.clone(),
                request: checkpoint.request.clone(),
                response: response.clone(),
                duration_ms: checkpoint.duration_ms,
            });
            return Ok(serde_json::json!({
                "bytes": bytes,
                "replay": true,
            }));
        }
        {
            let map = state.borrow_mut::<RuntimeStateMap>();
            if let Some(execution) = map.get_mut(&execution_id) {
                execution.boundary_stop = Some("tcp".to_string());
            }
        }
        tracing::debug!(%request_id, %call_index, host = %host, port = %port, "replay: stopping at tcp boundary (no recorded checkpoint)");
        return Err(JsErrorBox::generic(format!("__FLUX_BOUNDARY_STOP:tcp:{call_index}")));
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
    } = serde_json::from_value(request).map_err(|err| {
        JsErrorBox::type_error(format!("invalid postgres connect request: {err}"))
    })?;

    let target = parse_postgres_target(&connection_string)?;
    validate_outbound_host(
        &target.host,
        target.port,
        "postgres connect",
        "FLUXBASE_ALLOW_LOOPBACK_POSTGRES",
    )?;

    let (request_id, mode, session_id) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "Error",
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
                "Error",
                format!("op_flux_postgres_connect: execution_id '{execution_id}' disappeared during connect"),
            )
        })?;
        execution
            .postgres_sessions
            .insert(session_id.clone(), handle);
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

    let parsed_url = Url::parse(&connection_string).map_err(|err| {
        JsErrorBox::type_error(format!("invalid postgres connection string: {err}"))
    })?;
    if !matches!(parsed_url.scheme(), "postgres" | "postgresql") {
        return Err(JsErrorBox::type_error(
            "postgres connection string must use postgres:// or postgresql://",
        ));
    }
    let host = parsed_url
        .host_str()
        .ok_or_else(|| JsErrorBox::type_error("postgres connection string missing host"))?;
    let port = parsed_url.port().unwrap_or(5432);
    validate_outbound_host(
        host,
        port,
        "postgres connect",
        "FLUXBASE_ALLOW_LOOPBACK_POSTGRES",
    )?;

    let (request_id, call_index, mode, recorded_checkpoint) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "Error",
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
                        boundary: checkpoint.boundary.clone(),
                        url: checkpoint.url.clone(),
                        method: checkpoint.method.clone(),
                        request: checkpoint.request.clone(),
                        response: response.clone(),
                        duration_ms: checkpoint.duration_ms,
                    });
                }
            }
            tracing::debug!(%request_id, %call_index, host = %host, port = %port, "replay: returned recorded postgres query");
            print_checkpoint_replay(&FetchCheckpoint {
                call_index,
                boundary: checkpoint.boundary.clone(),
                url: checkpoint.url.clone(),
                method: checkpoint.method.clone(),
                request: checkpoint.request.clone(),
                response: response.clone(),
                duration_ms: checkpoint.duration_ms,
            });
            return Ok(postgres_checkpoint_response_json(&response, true))
        }
        {
            let map = state.borrow_mut::<RuntimeStateMap>();
            if let Some(execution) = map.get_mut(&execution_id) {
                execution.boundary_stop = Some("postgres".to_string());
            }
        }
        tracing::debug!(%request_id, %call_index, host = %host, port = %port, "replay: stopping at postgres boundary (no recorded checkpoint)");
        return Err(JsErrorBox::generic(format!("__FLUX_BOUNDARY_STOP:postgres:{call_index}")));
    }

    let started = std::time::Instant::now();
    let live_outcome = perform_postgres_simple_query(
        &connection_string,
        &sql,
        &PostgresTlsOptions {
            enabled: tls,
            ca_cert_pem,
        },
    )?;
    maybe_crash_after_postgres_commit_before_checkpoint(&sql);
    let duration_ms = started.elapsed().as_millis() as i32;

    let request_json = serde_json::json!({
        "url": format!("postgres://{}:{}/{}", host, port, parsed_url.path().trim_start_matches('/')),
        "host": host,
        "port": port,
        "sql": sql,
        "tls": tls,
    });
    let response_json = postgres_outcome_json(live_outcome, false);

    {
        let map = state.borrow_mut::<RuntimeStateMap>();
        if let Some(execution) = map.get_mut(&execution_id) {
            execution.checkpoints.push(FetchCheckpoint {
                call_index,
                boundary: "postgres".to_string(),
                url: format!(
                    "postgres://{}:{}/{}",
                    host,
                    port,
                    parsed_url.path().trim_start_matches('/')
                ),
                method: "simple_query".to_string(),
                request: request_json,
                response: response_json.clone(),
                duration_ms,
            });
        }
    }

    tracing::debug!(%request_id, %call_index, host = %host, port = %port, "intercepted postgres query");

    Ok(response_json)
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
    } = serde_json::from_value(request).map_err(|err| {
        JsErrorBox::type_error(format!("invalid postgres session query request: {err}"))
    })?;

    let (request_id, call_index, mode, recorded_checkpoint) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "Error",
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
                        boundary: checkpoint.boundary.clone(),
                        url: checkpoint.url.clone(),
                        method: checkpoint.method.clone(),
                        request: checkpoint.request.clone(),
                        response: response.clone(),
                        duration_ms: checkpoint.duration_ms,
                    });
                }
            }
            tracing::debug!(%request_id, %call_index, session_id = %session_id, "replay: returned recorded postgres session query");
            print_checkpoint_replay(&FetchCheckpoint {
                call_index,
                boundary: checkpoint.boundary.clone(),
                url: checkpoint.url.clone(),
                method: checkpoint.method.clone(),
                request: checkpoint.request.clone(),
                response: response.clone(),
                duration_ms: checkpoint.duration_ms,
            });
            return Ok(postgres_checkpoint_response_json(&response, true));
        }
        {
            let map = state.borrow_mut::<RuntimeStateMap>();
            if let Some(execution) = map.get_mut(&execution_id) {
                execution.boundary_stop = Some("postgres".to_string());
            }
        }
        tracing::debug!(%request_id, %call_index, session_id = %session_id, "replay: stopping at postgres boundary (no recorded checkpoint)");
        return Err(JsErrorBox::generic(format!("__FLUX_BOUNDARY_STOP:postgres:{call_index}")));
    }

    let (target, tls_enabled, live_outcome, duration_ms) = {
        let started = std::time::Instant::now();
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "Error",
                format!("op_flux_postgres_session_query: execution_id '{execution_id}' disappeared before query"),
            )
        })?;
        let session = execution
            .postgres_sessions
            .get(&session_id)
            .ok_or_else(|| {
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
    maybe_crash_after_postgres_commit_before_checkpoint(&sql);

    let request_json = serde_json::json!({
        "url": target.url,
        "host": target.host,
        "port": target.port,
        "sql": sql,
        "params": params,
        "tls": tls_enabled,
        "session": true,
    });
    let response_json = postgres_outcome_json(live_outcome, false);

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
        "fields": response_json.get("fields").cloned().unwrap_or_else(|| serde_json::json!([])),
        "command": response_json.get("command").cloned().unwrap_or(serde_json::Value::Null),
        "rowCount": response_json
            .get("rowCount")
            .cloned()
            .unwrap_or_else(|| serde_json::json!(0)),
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

    let parsed_url = Url::parse(&connection_string).map_err(|err| {
        JsErrorBox::type_error(format!("invalid postgres connection string: {err}"))
    })?;
    if !matches!(parsed_url.scheme(), "postgres" | "postgresql") {
        return Err(JsErrorBox::type_error(
            "postgres connection string must use postgres:// or postgresql://",
        ));
    }
    let host = parsed_url
        .host_str()
        .ok_or_else(|| JsErrorBox::type_error("postgres connection string missing host"))?;
    let port = parsed_url.port().unwrap_or(5432);
    validate_outbound_host(
        host,
        port,
        "postgres connect",
        "FLUXBASE_ALLOW_LOOPBACK_POSTGRES",
    )?;

    let (request_id, call_index, mode, recorded_checkpoint) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "Error",
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
                        boundary: checkpoint.boundary.clone(),
                        url: checkpoint.url.clone(),
                        method: checkpoint.method.clone(),
                        request: checkpoint.request.clone(),
                        response: response.clone(),
                        duration_ms: checkpoint.duration_ms,
                    });
                }
            }
            tracing::debug!(%request_id, %call_index, host = %host, port = %port, "replay: returned recorded postgres prepared query");
            print_checkpoint_replay(&FetchCheckpoint {
                call_index,
                boundary: checkpoint.boundary.clone(),
                url: checkpoint.url.clone(),
                method: checkpoint.method.clone(),
                request: checkpoint.request.clone(),
                response: response.clone(),
                duration_ms: checkpoint.duration_ms,
            });
            return Ok(postgres_checkpoint_response_json(&response, true));
        }
        {
            let map = state.borrow_mut::<RuntimeStateMap>();
            if let Some(execution) = map.get_mut(&execution_id) {
                execution.boundary_stop = Some("postgres".to_string());
            }
        }
        tracing::debug!(%request_id, %call_index, host = %host, port = %port, "replay: stopping at postgres boundary (no recorded checkpoint)");
        return Err(JsErrorBox::generic(format!("__FLUX_BOUNDARY_STOP:postgres:{call_index}")));
    }

    let started = std::time::Instant::now();
    let live_outcome = perform_postgres_query(
        &connection_string,
        &sql,
        &params,
        &PostgresTlsOptions {
            enabled: tls,
            ca_cert_pem,
        },
    )?;
    maybe_crash_after_postgres_commit_before_checkpoint(&sql);
    let duration_ms = started.elapsed().as_millis() as i32;

    let request_json = serde_json::json!({
        "url": format!("postgres://{}:{}/{}", host, port, parsed_url.path().trim_start_matches('/')),
        "host": host,
        "port": port,
        "sql": sql,
        "params": params,
        "tls": tls,
    });
    let response_json = postgres_outcome_json(live_outcome, false);

    {
        let map = state.borrow_mut::<RuntimeStateMap>();
        if let Some(execution) = map.get_mut(&execution_id) {
            execution.checkpoints.push(FetchCheckpoint {
                call_index,
                boundary: "postgres".to_string(),
                url: format!(
                    "postgres://{}:{}/{}",
                    host,
                    port,
                    parsed_url.path().trim_start_matches('/')
                ),
                method: "query".to_string(),
                request: request_json,
                response: response_json.clone(),
                duration_ms,
            });
        }
    }

    tracing::debug!(%request_id, %call_index, host = %host, port = %port, "intercepted postgres prepared query");

    Ok(response_json)
}

#[op2]
#[serde]
fn op_flux_redis_command(
    state: &mut OpState,
    #[serde] request: serde_json::Value,
) -> Result<serde_json::Value, JsErrorBox> {
    let RedisCommandRequestPayload {
        execution_id,
        connection_string,
        command,
        args,
    } = serde_json::from_value(request)
        .map_err(|err| JsErrorBox::type_error(format!("invalid redis command request: {err}")))?;

    let command = validate_redis_command(&command, &args)?;

    let target = parse_redis_target(&connection_string)?;
    validate_outbound_host(
        &target.host,
        target.port,
        "redis connect",
        "FLUXBASE_ALLOW_LOOPBACK_REDIS",
    )?;

    let (request_id, call_index, mode, recorded_checkpoint) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "Error",
                format!("op_flux_redis_command: execution_id '{execution_id}' not found"),
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
                        boundary: checkpoint.boundary.clone(),
                        url: checkpoint.url.clone(),
                        method: checkpoint.method.clone(),
                        request: checkpoint.request.clone(),
                        response: response.clone(),
                        duration_ms: checkpoint.duration_ms,
                    });
                }
            }
            tracing::debug!(%request_id, %call_index, command = %command, host = %target.host, port = %target.port, "replay: returned recorded redis command");
            print_checkpoint_replay(&FetchCheckpoint {
                call_index,
                boundary: checkpoint.boundary.clone(),
                url: checkpoint.url.clone(),
                method: checkpoint.method.clone(),
                request: checkpoint.request.clone(),
                response: response.clone(),
                duration_ms: checkpoint.duration_ms,
            });
            return Ok(redis_checkpoint_response_json(&response, true));
        }
        {
            let map = state.borrow_mut::<RuntimeStateMap>();
            if let Some(execution) = map.get_mut(&execution_id) {
                execution.boundary_stop = Some("redis".to_string());
            }
        }
        tracing::debug!(%request_id, %call_index, command = %command, host = %target.host, port = %target.port, "replay: stopping at redis boundary (no recorded checkpoint)");
        return Err(JsErrorBox::generic(format!("__FLUX_BOUNDARY_STOP:redis:{call_index}")));
    }

    let started = Instant::now();
    let outcome = perform_redis_command(&target, &command, &args)?;
    let duration_ms = started.elapsed().as_millis() as i32;

    let request_json = serde_json::json!({
        "url": target.url,
        "host": target.host,
        "port": target.port,
        "db": target.db,
        "command": command,
        "args": args,
    });
    let response_json = redis_outcome_json(outcome, false);

    {
        let map = state.borrow_mut::<RuntimeStateMap>();
        if let Some(execution) = map.get_mut(&execution_id) {
            execution.checkpoints.push(FetchCheckpoint {
                call_index,
                boundary: "redis".to_string(),
                url: target.url.clone(),
                method: command.clone(),
                request: request_json,
                response: response_json.clone(),
                duration_ms,
            });
        }
    }

    tracing::debug!(%request_id, %call_index, command = %command, host = %target.host, port = %target.port, "intercepted redis command");

    Ok(redis_checkpoint_response_json(&response_json, false))
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
    } = serde_json::from_value(request).map_err(|err| {
        JsErrorBox::type_error(format!("invalid postgres session close request: {err}"))
    })?;

    let (request_id, mode) = {
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.get_mut(&execution_id).ok_or_else(|| {
            JsErrorBox::new(
                "Error",
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
                "Error",
                format!("op_flux_postgres_close_session: execution_id '{execution_id}' disappeared during close"),
            )
        })?;
        execution.postgres_sessions.remove(&session_id)
    };

    tracing::debug!(%request_id, session_id = %session_id, closed = removed.is_some(), "closed postgres session");

    Ok(serde_json::json!({ "closed": removed.is_some(), "replay": false }))
}

#[derive(Debug)]
struct PostgresFieldMetadata {
    name: String,
    data_type_id: u32,
    format: String,
}

struct PostgresNumericText(String);

struct PostgresJsonText(String);

struct PostgresArrayText(String);

struct PostgresTextValue(String);

struct PostgresByteaText(String);

impl<'a> FromSql<'a> for PostgresNumericText {
    fn from_sql(
        _ty: &PostgresType,
        raw: &'a [u8],
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(std::str::from_utf8(raw)?.to_string()))
    }

    fn accepts(ty: &PostgresType) -> bool {
        *ty == postgres::types::Type::NUMERIC
    }
}

impl<'a> FromSql<'a> for PostgresJsonText {
    fn from_sql(
        _ty: &PostgresType,
        raw: &'a [u8],
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(std::str::from_utf8(raw)?.to_string()))
    }

    fn accepts(ty: &PostgresType) -> bool {
        *ty == postgres::types::Type::JSON || *ty == postgres::types::Type::JSONB
    }
}

impl<'a> FromSql<'a> for PostgresArrayText {
    fn from_sql(
        _ty: &PostgresType,
        raw: &'a [u8],
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(std::str::from_utf8(raw)?.to_string()))
    }

    fn accepts(ty: &PostgresType) -> bool {
        matches!(
            *ty,
            postgres::types::Type::BOOL_ARRAY
                | postgres::types::Type::INT2_ARRAY
                | postgres::types::Type::INT4_ARRAY
                | postgres::types::Type::INT8_ARRAY
                | postgres::types::Type::FLOAT4_ARRAY
                | postgres::types::Type::FLOAT8_ARRAY
                | postgres::types::Type::TEXT_ARRAY
                | postgres::types::Type::VARCHAR_ARRAY
                | postgres::types::Type::NUMERIC_ARRAY
        )
    }
}

impl<'a> FromSql<'a> for PostgresTextValue {
    fn from_sql(
        _ty: &PostgresType,
        raw: &'a [u8],
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(std::str::from_utf8(raw)?.to_string()))
    }

    fn accepts(ty: &PostgresType) -> bool {
        matches!(
            *ty,
            postgres::types::Type::DATE
                | postgres::types::Type::TIME
                | postgres::types::Type::TIMETZ
                | postgres::types::Type::TIMESTAMP
                | postgres::types::Type::TIMESTAMPTZ
                | postgres::types::Type::INTERVAL
                | postgres::types::Type::OID
                | postgres::types::Type::UUID
        )
    }
}

impl<'a> FromSql<'a> for PostgresByteaText {
    fn from_sql(
        _ty: &PostgresType,
        raw: &'a [u8],
    ) -> std::result::Result<Self, Box<dyn std::error::Error + Sync + Send>> {
        Ok(Self(std::str::from_utf8(raw)?.to_string()))
    }

    fn accepts(ty: &PostgresType) -> bool {
        *ty == postgres::types::Type::BYTEA
    }
}

#[derive(Debug)]
struct PostgresSimpleQueryResponse {
    rows: Vec<serde_json::Value>,
    fields: Vec<PostgresFieldMetadata>,
    command: Option<String>,
    row_count: usize,
}

#[derive(Debug, Clone, Copy)]
enum PostgresJsonNumberParam {
    Integer(i64),
    Unsigned(u64),
    Float(f64),
}

impl ToSql for PostgresJsonNumberParam {
    fn to_sql(
        &self,
        ty: &PostgresType,
        out: &mut BytesMut,
    ) -> std::result::Result<IsNull, Box<dyn std::error::Error + Sync + Send>> {
        match self {
            Self::Integer(value) => match *ty {
                PostgresType::INT2 => i16::try_from(*value)?.to_sql(ty, out),
                PostgresType::INT4 => i32::try_from(*value)?.to_sql(ty, out),
                PostgresType::INT8 => value.to_sql(ty, out),
                PostgresType::FLOAT4 => (*value as f32).to_sql(ty, out),
                PostgresType::FLOAT8 => (*value as f64).to_sql(ty, out),
                _ => Err(
                    format!("unsupported postgres integer parameter type: {}", ty.name()).into(),
                ),
            },
            Self::Unsigned(value) => match *ty {
                PostgresType::INT2 => i16::try_from(*value)?.to_sql(ty, out),
                PostgresType::INT4 => i32::try_from(*value)?.to_sql(ty, out),
                PostgresType::INT8 => i64::try_from(*value)?.to_sql(ty, out),
                PostgresType::FLOAT4 => (*value as f32).to_sql(ty, out),
                PostgresType::FLOAT8 => (*value as f64).to_sql(ty, out),
                _ => Err(
                    format!("unsupported postgres integer parameter type: {}", ty.name()).into(),
                ),
            },
            Self::Float(value) => match *ty {
                PostgresType::FLOAT4 => (*value as f32).to_sql(ty, out),
                PostgresType::FLOAT8 => value.to_sql(ty, out),
                _ => {
                    Err(format!("unsupported postgres float parameter type: {}", ty.name()).into())
                }
            },
        }
    }

    fn accepts(ty: &PostgresType) -> bool {
        matches!(
            *ty,
            PostgresType::INT2
                | PostgresType::INT4
                | PostgresType::INT8
                | PostgresType::FLOAT4
                | PostgresType::FLOAT8
        )
    }

    postgres::types::to_sql_checked!();
}

fn perform_postgres_simple_query(
    connection_string: &str,
    sql: &str,
    tls: &PostgresTlsOptions,
) -> Result<PostgresQueryOutcome, JsErrorBox> {
    let connection_string = connection_string.to_string();
    let sql = sql.to_string();
    let tls = tls.clone();
    std::thread::spawn(move || {
        let mut client = connect_postgres_client(&connection_string, &tls)?;
        Ok(
            match perform_postgres_simple_query_with_client(&mut client, &sql) {
                Ok(response) => PostgresQueryOutcome::Success(response),
                Err(err) => PostgresQueryOutcome::Error(err),
            },
        )
    })
    .join()
    .map_err(|_| JsErrorBox::generic("postgres query thread panicked"))?
}

fn perform_postgres_query(
    connection_string: &str,
    sql: &str,
    params: &[serde_json::Value],
    tls: &PostgresTlsOptions,
) -> Result<PostgresQueryOutcome, JsErrorBox> {
    let connection_string = connection_string.to_string();
    let sql = sql.to_string();
    let params = params.to_vec();
    let tls = tls.clone();
    std::thread::spawn(move || {
        let mut client = connect_postgres_client(&connection_string, &tls)?;
        Ok(
            match perform_postgres_query_with_client(&mut client, &sql, &params) {
                Ok(response) => PostgresQueryOutcome::Success(response),
                Err(err) => PostgresQueryOutcome::Error(err),
            },
        )
    })
    .join()
    .map_err(|_| JsErrorBox::generic("postgres query thread panicked"))?
}

fn perform_postgres_simple_query_with_client(
    client: &mut PostgresClient,
    sql: &str,
) -> std::result::Result<PostgresSimpleQueryResponse, PostgresDriverError> {
    let messages = client
        .simple_query(sql)
        .map_err(|err| PostgresDriverError::from_postgres_error(err, "postgres query failed"))?;

    let mut rows = Vec::new();
    let mut fields = Vec::new();
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
            SimpleQueryMessage::RowDescription(columns) => {
                fields = columns
                    .iter()
                    .map(|column| PostgresFieldMetadata {
                        name: column.name().to_string(),
                        data_type_id: postgres::types::Type::TEXT.oid(),
                        format: "text".to_string(),
                    })
                    .collect();
            }
            _ => {}
        }
    }

    Ok(PostgresSimpleQueryResponse {
        row_count: rows.len(),
        rows,
        fields,
        command,
    })
}

fn perform_postgres_query_with_client(
    client: &mut PostgresClient,
    sql: &str,
    params: &[serde_json::Value],
) -> std::result::Result<PostgresSimpleQueryResponse, PostgresDriverError> {
    let mut boxed_params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
    for param in params.iter().cloned() {
        boxed_params.push(
            box_postgres_param(param).map_err(|err| PostgresDriverError {
                message: err.to_string(),
                code: None,
                detail: None,
                constraint: None,
                schema: None,
                table: None,
                column: None,
            })?,
        );
    }
    let refs: Vec<&(dyn ToSql + Sync)> = boxed_params
        .iter()
        .map(|value| value.as_ref() as &(dyn ToSql + Sync))
        .collect();

    let query_rows = client
        .query(sql, &refs)
        .map_err(|err| PostgresDriverError::from_postgres_error(err, "postgres query failed"))?;

    let fields = query_rows
        .first()
        .map(|row| {
            row.columns()
                .iter()
                .map(|column| PostgresFieldMetadata {
                    name: column.name().to_string(),
                    data_type_id: column.type_().oid(),
                    format: "text".to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
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
        fields,
        command: Some("QUERY".to_string()),
    })
}

fn postgres_success_json(response: PostgresSimpleQueryResponse, replay: bool) -> serde_json::Value {
    serde_json::json!({
        "rows": response.rows,
        "fields": postgres_fields_json(&response.fields),
        "command": response.command,
        "rowCount": response.row_count,
        "row_count": response.row_count,
        "error": serde_json::Value::Null,
        "replay": replay,
    })
}

fn postgres_error_json(error: PostgresDriverError, replay: bool) -> serde_json::Value {
    serde_json::json!({
        "rows": [],
        "fields": [],
        "command": serde_json::Value::Null,
        "rowCount": 0,
        "row_count": 0,
        "error": error.to_json(),
        "replay": replay,
    })
}

fn postgres_outcome_json(outcome: PostgresQueryOutcome, replay: bool) -> serde_json::Value {
    match outcome {
        PostgresQueryOutcome::Success(response) => postgres_success_json(response, replay),
        PostgresQueryOutcome::Error(error) => postgres_error_json(error, replay),
    }
}

fn postgres_checkpoint_response_json(
    response: &serde_json::Value,
    replay: bool,
) -> serde_json::Value {
    let recorded_error = response
        .get("error")
        .cloned()
        .unwrap_or(serde_json::Value::Null);
    if !recorded_error.is_null() {
        serde_json::json!({
            "rows": [],
            "fields": [],
            "command": serde_json::Value::Null,
            "rowCount": 0,
            "row_count": 0,
            "error": recorded_error,
            "replay": replay,
        })
    } else {
        serde_json::json!({
            "rows": response.get("rows").cloned().unwrap_or_else(|| serde_json::json!([])),
            "fields": response.get("fields").cloned().unwrap_or_else(|| serde_json::json!([])),
            "command": response.get("command").cloned().unwrap_or(serde_json::Value::Null),
            "rowCount": response
                .get("rowCount")
                .cloned()
                .or_else(|| response.get("row_count").cloned())
                .unwrap_or_else(|| serde_json::json!(0)),
            "row_count": response
                .get("row_count")
                .cloned()
                .or_else(|| response.get("rowCount").cloned())
                .unwrap_or_else(|| serde_json::json!(0)),
            "error": serde_json::Value::Null,
            "replay": replay,
        })
    }
}

fn emit_flux_event(event: serde_json::Value) {
    println!("[flux-event] {}", event.to_string());
}

fn print_checkpoint_replay(checkpoint: &FetchCheckpoint) {
    let cp = checkpoint;
    let boundary = cp.boundary.to_ascii_uppercase();

    match cp.boundary.as_str() {
        "postgres" => {
            let sql = cp.request.get("sql").and_then(|v| v.as_str()).unwrap_or("");
            let host = cp.request.get("host").and_then(|v| v.as_str()).unwrap_or("");
            let row_count = cp
                .response
                .get("row_count")
                .or_else(|| cp.response.get("rowCount"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            
            // Structured event for CLI
            emit_flux_event(serde_json::json!({
                "type": "db_query",
                "query": sql,
                "duration_ms": cp.duration_ms,
            }));

            // Legacy fallback for old CLI versions
            println!(
                "  \x1b[32m✓\x1b[0m \x1b[1mPOSTGRES\x1b[0m  {}  {}ms  → {} rows  \x1b[2m{}\x1b[0m",
                host, cp.duration_ms, row_count, sql
            );
        }
        "http" => {
            let method = if !cp.method.is_empty() {
                cp.method.to_ascii_uppercase()
            } else {
                cp.request
                    .get("method")
                    .and_then(|v| v.as_str())
                    .unwrap_or("GET")
                    .to_string()
            };
            let status = cp
                .response
                .get("status")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            
            // Structured event for CLI
            emit_flux_event(serde_json::json!({
                "type": "fetch_end",
                "method": method,
                "status": status,
                "duration_ms": cp.duration_ms,
            }));

            let status_color = if status < 400 { "\x1b[32m" } else { "\x1b[31m" };
            println!(
                "  \x1b[32m✓\x1b[0m \x1b[1mHTTP\x1b[0m  {} {}  {}ms  → {}{}\x1b[0m",
                method, cp.url, cp.duration_ms, status_color, status
            );
        }
        "redis" => {
            let command = cp
                .request
                .get("command")
                .and_then(|v| v.as_str())
                .unwrap_or("COMMAND");
            println!(
                "  \x1b[32m✓\x1b[0m \x1b[1mREDIS\x1b[0m  {}  {}ms",
                command, cp.duration_ms
            );
        }
        "timer" | "performance.now" => {
            println!(
                "  \x1b[2m›\x1b[0m \x1b[1m{}\x1b[0m  {}ms",
                boundary, cp.duration_ms
            );
        }
        _ => {
            println!(
                "  \x1b[2m›\x1b[0m \x1b[1m{}\x1b[0m  {}  {}ms",
                boundary, cp.url, cp.duration_ms
            );
        }
    }
}

fn parse_postgres_target(connection_string: &str) -> Result<PostgresConnectionTarget, JsErrorBox> {
    let parsed_url = Url::parse(connection_string).map_err(|err| {
        JsErrorBox::type_error(format!("invalid postgres connection string: {err}"))
    })?;
    if !matches!(parsed_url.scheme(), "postgres" | "postgresql") {
        return Err(JsErrorBox::type_error(
            "postgres connection string must use postgres:// or postgresql://",
        ));
    }
    let host = parsed_url
        .host_str()
        .ok_or_else(|| JsErrorBox::type_error("postgres connection string missing host"))?
        .to_string();
    let port = parsed_url.port().unwrap_or(5432);

    Ok(PostgresConnectionTarget {
        url: format!(
            "postgres://{}:{}/{}",
            host,
            port,
            parsed_url.path().trim_start_matches('/')
        ),
        host,
        port,
    })
}

fn redis_success_json(value: serde_json::Value, replay: bool) -> serde_json::Value {
    serde_json::json!({
        "value": value,
        "error": serde_json::Value::Null,
        "replay": replay,
    })
}

fn redis_error_json(error: RedisDriverError, replay: bool) -> serde_json::Value {
    serde_json::json!({
        "value": serde_json::Value::Null,
        "error": error.to_json(),
        "replay": replay,
    })
}

fn redis_outcome_json(outcome: RedisCommandOutcome, replay: bool) -> serde_json::Value {
    match outcome {
        RedisCommandOutcome::Success(value) => redis_success_json(value, replay),
        RedisCommandOutcome::Error(error) => redis_error_json(error, replay),
    }
}

fn redis_checkpoint_response_json(response: &serde_json::Value, replay: bool) -> serde_json::Value {
    serde_json::json!({
        "value": response.get("value").cloned().unwrap_or(serde_json::Value::Null),
        "error": response.get("error").cloned().unwrap_or(serde_json::Value::Null),
        "replay": replay,
    })
}

fn parse_redis_target(connection_string: &str) -> Result<RedisConnectionTarget, JsErrorBox> {
    let parsed_url = Url::parse(connection_string)
        .map_err(|err| JsErrorBox::type_error(format!("invalid redis connection string: {err}")))?;
    if parsed_url.scheme() != "redis" {
        return Err(JsErrorBox::type_error(
            "redis connection string must use redis://",
        ));
    }

    let host = parsed_url
        .host_str()
        .ok_or_else(|| JsErrorBox::type_error("redis connection string missing host"))?
        .to_string();
    let port = parsed_url.port().unwrap_or(6379);
    let db = match parsed_url.path().trim_start_matches('/') {
        "" => 0,
        raw_db => raw_db.parse::<i64>().map_err(|_| {
            JsErrorBox::type_error("redis connection string path must be a numeric database index")
        })?,
    };
    let username = if parsed_url.username().is_empty() {
        None
    } else {
        Some(parsed_url.username().to_string())
    };
    let password = parsed_url.password().map(str::to_string);

    Ok(RedisConnectionTarget {
        url: format!("redis://{}:{}/{}", host, port, db),
        host,
        port,
        db,
        username,
        password,
    })
}

fn blocked_redis_feature_error(feature: &str) -> JsErrorBox {
    let verb = match feature {
        "pub/sub" => "is",
        _ => "are",
    };
    JsErrorBox::generic(format!(
        "Redis {feature} {verb} not supported in Flux (non-deterministic execution)"
    ))
}

fn normalize_redis_command(command: &str) -> Result<String, JsErrorBox> {
    let normalized = command.trim().to_ascii_uppercase();
    if normalized.is_empty() {
        return Err(JsErrorBox::type_error("redis command is required"));
    }
    Ok(normalized)
}

fn validate_redis_command(command: &str, args: &[serde_json::Value]) -> Result<String, JsErrorBox> {
    let normalized = normalize_redis_command(command)?;

    match normalized.as_str() {
        "MULTI" | "EXEC" | "WATCH" | "UNWATCH" => {
            return Err(blocked_redis_feature_error("transactions"));
        }
        "SUBSCRIBE" | "PSUBSCRIBE" | "UNSUBSCRIBE" | "PUNSUBSCRIBE" | "PUBLISH" => {
            return Err(blocked_redis_feature_error("pub/sub"));
        }
        "BLPOP" | "BRPOP" | "BZPOPMIN" | "BZPOPMAX" => {
            return Err(blocked_redis_feature_error("blocking commands"));
        }
        "XREAD" | "XREADGROUP" => {
            let uses_block = args.iter().any(|value| {
                value
                    .as_str()
                    .map(|text| text.eq_ignore_ascii_case("BLOCK"))
                    .unwrap_or(false)
            });
            if uses_block {
                return Err(blocked_redis_feature_error("blocking commands"));
            }
        }
        _ => {}
    }

    Ok(normalized)
}

fn perform_redis_command(
    target: &RedisConnectionTarget,
    command: &str,
    args: &[serde_json::Value],
) -> Result<RedisCommandOutcome, JsErrorBox> {
    let target = target.clone();
    let command = normalize_redis_command(command)?;
    let args = args.to_vec();
    std::thread::spawn(move || {
        let mut stream = connect_redis_stream(&target)?;

        if let Some(password) = target.password.as_deref() {
            let auth_args = if let Some(username) = target.username.as_deref() {
                vec![username.as_bytes().to_vec(), password.as_bytes().to_vec()]
            } else {
                vec![password.as_bytes().to_vec()]
            };
            if let Err(error) =
                ensure_redis_ok(send_redis_command(&mut stream, "AUTH", &auth_args)?)
            {
                return Ok(RedisCommandOutcome::Error(error));
            }
        }

        if target.db != 0 {
            if let Err(error) = ensure_redis_ok(send_redis_command(
                &mut stream,
                "SELECT",
                &[target.db.to_string().into_bytes()],
            )?) {
                return Ok(RedisCommandOutcome::Error(error));
            }
        }

        let encoded_args = args
            .iter()
            .map(redis_argument_bytes)
            .collect::<Result<Vec<_>, _>>()?;

        match send_redis_command(&mut stream, &command, &encoded_args)? {
            RedisRespValue::Error(error) => {
                Ok(RedisCommandOutcome::Error(RedisDriverError::new(error)))
            }
            response => Ok(RedisCommandOutcome::Success(redis_resp_to_json(response))),
        }
    })
    .join()
    .map_err(|_| JsErrorBox::generic("redis command thread panicked"))?
}

fn connect_redis_stream(target: &RedisConnectionTarget) -> Result<TcpStream, JsErrorBox> {
    let address = (target.host.as_str(), target.port)
        .to_socket_addrs()
        .map_err(|err| JsErrorBox::type_error(format!("redis connect failed: {err}")))?
        .next()
        .ok_or_else(|| {
            JsErrorBox::type_error("redis connect failed: no socket addresses resolved")
        })?;

    let stream =
        TcpStream::connect_timeout(&address, Duration::from_millis(DEFAULT_CONNECT_TIMEOUT_MS))
            .map_err(|err| JsErrorBox::type_error(format!("redis connect failed: {err}")))?;
    stream
        .set_read_timeout(Some(Duration::from_millis(DEFAULT_READ_TIMEOUT_MS)))
        .map_err(|err| JsErrorBox::type_error(format!("redis read timeout failed: {err}")))?;
    stream
        .set_write_timeout(Some(Duration::from_millis(DEFAULT_READ_TIMEOUT_MS)))
        .map_err(|err| JsErrorBox::type_error(format!("redis write timeout failed: {err}")))?;

    Ok(stream)
}

fn redis_argument_bytes(value: &serde_json::Value) -> Result<Vec<u8>, JsErrorBox> {
    match value {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::String(value) => Ok(value.as_bytes().to_vec()),
        serde_json::Value::Bool(value) => Ok(if *value {
            b"true".to_vec()
        } else {
            b"false".to_vec()
        }),
        serde_json::Value::Number(value) => Ok(value.to_string().into_bytes()),
        other => Err(JsErrorBox::type_error(format!(
            "unsupported redis argument type: {other}"
        ))),
    }
}

#[derive(Debug, Clone)]
enum RedisRespValue {
    SimpleString(String),
    BulkString(Option<Vec<u8>>),
    Integer(i64),
    Array(Option<Vec<RedisRespValue>>),
    Error(String),
}

fn send_redis_command(
    stream: &mut TcpStream,
    command: &str,
    args: &[Vec<u8>],
) -> Result<RedisRespValue, JsErrorBox> {
    let mut payload = Vec::new();
    payload.extend_from_slice(format!("*{}\r\n", args.len() + 1).as_bytes());
    payload.extend_from_slice(format!("${}\r\n", command.len()).as_bytes());
    payload.extend_from_slice(command.as_bytes());
    payload.extend_from_slice(b"\r\n");
    for arg in args {
        payload.extend_from_slice(format!("${}\r\n", arg.len()).as_bytes());
        payload.extend_from_slice(arg);
        payload.extend_from_slice(b"\r\n");
    }
    stream
        .write_all(&payload)
        .map_err(|err| JsErrorBox::type_error(format!("redis write failed: {err}")))?;
    stream
        .flush()
        .map_err(|err| JsErrorBox::type_error(format!("redis flush failed: {err}")))?;

    read_redis_response(stream)
        .map_err(|err| JsErrorBox::type_error(format!("redis read failed: {err}")))
}

fn ensure_redis_ok(response: RedisRespValue) -> Result<(), RedisDriverError> {
    match response {
        RedisRespValue::SimpleString(value) if value.eq_ignore_ascii_case("OK") => Ok(()),
        RedisRespValue::Error(message) => Err(RedisDriverError::new(message)),
        other => Err(RedisDriverError::new(format!(
            "unexpected redis response: {:?}",
            other
        ))),
    }
}

fn read_redis_response(stream: &mut TcpStream) -> Result<RedisRespValue, String> {
    let mut prefix = [0u8; 1];
    stream
        .read_exact(&mut prefix)
        .map_err(|err| err.to_string())?;
    match prefix[0] {
        b'+' => Ok(RedisRespValue::SimpleString(read_redis_line(stream)?)),
        b'-' => Ok(RedisRespValue::Error(read_redis_line(stream)?)),
        b':' => read_redis_line(stream)?
            .parse::<i64>()
            .map(RedisRespValue::Integer)
            .map_err(|err| err.to_string()),
        b'$' => {
            let len = read_redis_line(stream)?
                .parse::<isize>()
                .map_err(|err| err.to_string())?;
            if len < 0 {
                return Ok(RedisRespValue::BulkString(None));
            }
            let mut bytes = vec![0u8; len as usize];
            stream
                .read_exact(&mut bytes)
                .map_err(|err| err.to_string())?;
            consume_redis_crlf(stream)?;
            Ok(RedisRespValue::BulkString(Some(bytes)))
        }
        b'*' => {
            let count = read_redis_line(stream)?
                .parse::<isize>()
                .map_err(|err| err.to_string())?;
            if count < 0 {
                return Ok(RedisRespValue::Array(None));
            }
            let mut items = Vec::with_capacity(count as usize);
            for _ in 0..count {
                items.push(read_redis_response(stream)?);
            }
            Ok(RedisRespValue::Array(Some(items)))
        }
        other => Err(format!(
            "unsupported redis response prefix: {}",
            other as char
        )),
    }
}

fn read_redis_line(stream: &mut TcpStream) -> Result<String, String> {
    let mut bytes = Vec::new();
    loop {
        let mut next = [0u8; 1];
        stream
            .read_exact(&mut next)
            .map_err(|err| err.to_string())?;
        if next[0] == b'\r' {
            let mut lf = [0u8; 1];
            stream.read_exact(&mut lf).map_err(|err| err.to_string())?;
            if lf[0] != b'\n' {
                return Err("invalid redis line ending".to_string());
            }
            break;
        }
        bytes.push(next[0]);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn consume_redis_crlf(stream: &mut TcpStream) -> Result<(), String> {
    let mut suffix = [0u8; 2];
    stream
        .read_exact(&mut suffix)
        .map_err(|err| err.to_string())?;
    if suffix != [b'\r', b'\n'] {
        return Err("invalid redis bulk-string terminator".to_string());
    }
    Ok(())
}

fn redis_resp_to_json(value: RedisRespValue) -> serde_json::Value {
    match value {
        RedisRespValue::SimpleString(value) => serde_json::Value::String(value),
        RedisRespValue::BulkString(Some(bytes)) => {
            serde_json::Value::String(String::from_utf8_lossy(&bytes).into_owned())
        }
        RedisRespValue::BulkString(None) => serde_json::Value::Null,
        RedisRespValue::Integer(value) => serde_json::json!(value),
        RedisRespValue::Array(Some(values)) => {
            serde_json::Value::Array(values.into_iter().map(redis_resp_to_json).collect())
        }
        RedisRespValue::Array(None) => serde_json::Value::Null,
        RedisRespValue::Error(message) => serde_json::json!({ "error": message }),
    }
}

fn connect_postgres_client(
    connection_string: &str,
    tls: &PostgresTlsOptions,
) -> Result<PostgresClient, JsErrorBox> {
    let mut config: PostgresConfig = connection_string.parse().map_err(|err| {
        JsErrorBox::type_error(format!("invalid postgres connection string: {err}"))
    })?;

    let use_tls = tls.enabled || matches!(config.get_ssl_mode(), PostgresSslMode::Require);

    if use_tls {
        // Ensure we're at least in Require mode or better if using a TLS connector
        if config.get_ssl_mode() == PostgresSslMode::Disable
            || config.get_ssl_mode() == PostgresSslMode::Prefer
        {
            config.ssl_mode(PostgresSslMode::Require);
        }
        
        let tls_config = build_tls_client_config(tls.ca_cert_pem.as_deref())?;
        let connector = PostgresMakeTlsConnector::new(TlsConnector::from(tls_config));
        config
            .connect(connector)
            .map_err(|err| {
                let msg = if let Some(db_err) = err.as_db_error() {
                    format!("postgres connect failed: {} (code: {})", db_err.message(), db_err.code().code())
                } else if err.to_string().contains("Connection refused") {
                    format!("postgres connect failed: Connection refused. Is Postgres running and accessible at the provided URL?")
                } else if err.to_string().contains("timed out") {
                    format!("postgres connect failed: Connection timed out. Check your network and DATABASE_URL.")
                } else {
                    format!("postgres connect failed: {err}")
                };
                JsErrorBox::type_error(msg)
            })
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
                Ok(Box::new(PostgresJsonNumberParam::Integer(signed)))
            } else if let Some(unsigned) = value.as_u64() {
                Ok(Box::new(PostgresJsonNumberParam::Unsigned(unsigned)))
            } else if let Some(float) = value.as_f64() {
                Ok(Box::new(PostgresJsonNumberParam::Float(float)))
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
    let string_value = || row.try_get::<_, Option<String>>(name).ok().flatten();

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
            .or_else(|| {
                string_value().and_then(|value| {
                    value
                        .parse::<i16>()
                        .ok()
                        .map(|parsed| serde_json::json!(parsed))
                })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INT4 => row
            .try_get::<_, Option<i32>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::json!(value))
            .or_else(|| {
                string_value().and_then(|value| {
                    value
                        .parse::<i32>()
                        .ok()
                        .map(|parsed| serde_json::json!(parsed))
                })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INT8 => row
            .try_get::<_, Option<i64>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::json!(value))
            .or_else(|| {
                string_value().and_then(|value| {
                    value
                        .parse::<i64>()
                        .ok()
                        .map(|parsed| serde_json::json!(parsed))
                })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::FLOAT4 => row
            .try_get::<_, Option<f32>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::json!(value))
            .or_else(|| {
                string_value().and_then(|value| {
                    value
                        .parse::<f32>()
                        .ok()
                        .map(|parsed| serde_json::json!(parsed))
                })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::FLOAT8 => row
            .try_get::<_, Option<f64>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::json!(value))
            .or_else(|| {
                string_value().and_then(|value| {
                    value
                        .parse::<f64>()
                        .ok()
                        .map(|parsed| serde_json::json!(parsed))
                })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::OID => row
            .try_get::<_, Option<u32>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::json!(value))
            .or_else(|| {
                row.try_get::<_, Option<PostgresTextValue>>(name)
                    .ok()
                    .flatten()
                    .and_then(|value| {
                        value
                            .0
                            .parse::<u32>()
                            .ok()
                            .map(|parsed| serde_json::json!(parsed))
                    })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::BYTEA => row
            .try_get::<_, Option<PostgresByteaText>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::Value::String(value.0))
            .or_else(|| {
                row.try_get::<_, Option<Vec<u8>>>(name)
                    .ok()
                    .flatten()
                    .map(|value| serde_json::Value::String(format!("\\x{}", hex::encode(value))))
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::NUMERIC => row
            .try_get::<_, Option<PostgresNumericText>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::Value::String(value.0))
            .or_else(|| string_value().map(serde_json::Value::String))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::DATE => row
            .try_get::<_, Option<NaiveDate>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::Value::String(value.format("%Y-%m-%d").to_string()))
            .or_else(|| {
                row.try_get::<_, Option<PostgresTextValue>>(name)
                    .ok()
                    .flatten()
                    .map(|value| serde_json::Value::String(value.0))
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::TIME => row
            .try_get::<_, Option<NaiveTime>>(name)
            .ok()
            .flatten()
            .map(|value| {
                serde_json::Value::String(
                    value
                        .format("%H:%M:%S%.f")
                        .to_string()
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_string(),
                )
            })
            .or_else(|| {
                row.try_get::<_, Option<PostgresTextValue>>(name)
                    .ok()
                    .flatten()
                    .map(|value| serde_json::Value::String(value.0))
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::TIMETZ => row
            .try_get::<_, Option<PostgresTextValue>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::Value::String(value.0))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::TIMESTAMP => row
            .try_get::<_, Option<NaiveDateTime>>(name)
            .ok()
            .flatten()
            .map(|value| {
                serde_json::Value::String(
                    value
                        .format("%Y-%m-%dT%H:%M:%S%.f")
                        .to_string()
                        .trim_end_matches('0')
                        .trim_end_matches('.')
                        .to_string(),
                )
            })
            .or_else(|| {
                row.try_get::<_, Option<PostgresTextValue>>(name)
                    .ok()
                    .flatten()
                    .map(|value| serde_json::Value::String(value.0))
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::TIMESTAMPTZ => row
            .try_get::<_, Option<DateTime<Utc>>>(name)
            .ok()
            .flatten()
            .map(|value| {
                serde_json::Value::String(value.to_rfc3339_opts(SecondsFormat::Millis, true))
            })
            .or_else(|| {
                row.try_get::<_, Option<PostgresTextValue>>(name)
                    .ok()
                    .flatten()
                    .map(|value| serde_json::Value::String(value.0))
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INTERVAL => row
            .try_get::<_, Option<PostgresTextValue>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::Value::String(value.0))
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::UUID => row
            .try_get::<_, Option<Uuid>>(name)
            .ok()
            .flatten()
            .map(|value| serde_json::Value::String(value.to_string()))
            .or_else(|| {
                row.try_get::<_, Option<PostgresTextValue>>(name)
                    .ok()
                    .flatten()
                    .map(|value| serde_json::Value::String(value.0))
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::JSON | postgres::types::Type::JSONB => row
            .try_get::<_, Option<serde_json::Value>>(name)
            .ok()
            .flatten()
            .or_else(|| {
                row.try_get::<_, Option<PostgresJsonText>>(name)
                    .ok()
                    .flatten()
                    .and_then(|value| serde_json::from_str::<serde_json::Value>(&value.0).ok())
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::BOOL_ARRAY => row
            .try_get::<_, Option<Vec<bool>>>(name)
            .ok()
            .flatten()
            .map(|values| {
                serde_json::Value::Array(values.into_iter().map(serde_json::Value::Bool).collect())
            })
            .or_else(|| {
                row.try_get::<_, Option<PostgresArrayText>>(name)
                    .ok()
                    .flatten()
                    .and_then(|value| {
                        parse_postgres_text_array(&value.0, parse_postgres_bool_array_element)
                    })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INT2_ARRAY => row
            .try_get::<_, Option<Vec<i16>>>(name)
            .ok()
            .flatten()
            .map(|values| {
                serde_json::Value::Array(
                    values
                        .into_iter()
                        .map(|value| serde_json::json!(value))
                        .collect(),
                )
            })
            .or_else(|| {
                row.try_get::<_, Option<PostgresArrayText>>(name)
                    .ok()
                    .flatten()
                    .and_then(|value| {
                        parse_postgres_text_array(&value.0, |item| {
                            item.parse::<i16>()
                                .ok()
                                .map(|parsed| serde_json::json!(parsed))
                        })
                    })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INT4_ARRAY => row
            .try_get::<_, Option<Vec<i32>>>(name)
            .ok()
            .flatten()
            .map(|values| {
                serde_json::Value::Array(
                    values
                        .into_iter()
                        .map(|value| serde_json::json!(value))
                        .collect(),
                )
            })
            .or_else(|| {
                row.try_get::<_, Option<PostgresArrayText>>(name)
                    .ok()
                    .flatten()
                    .and_then(|value| {
                        parse_postgres_text_array(&value.0, |item| {
                            item.parse::<i32>()
                                .ok()
                                .map(|parsed| serde_json::json!(parsed))
                        })
                    })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::INT8_ARRAY => row
            .try_get::<_, Option<Vec<i64>>>(name)
            .ok()
            .flatten()
            .map(|values| {
                serde_json::Value::Array(
                    values
                        .into_iter()
                        .map(|value| serde_json::json!(value))
                        .collect(),
                )
            })
            .or_else(|| {
                row.try_get::<_, Option<PostgresArrayText>>(name)
                    .ok()
                    .flatten()
                    .and_then(|value| {
                        parse_postgres_text_array(&value.0, |item| {
                            item.parse::<i64>()
                                .ok()
                                .map(|parsed| serde_json::json!(parsed))
                        })
                    })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::FLOAT4_ARRAY => row
            .try_get::<_, Option<Vec<f32>>>(name)
            .ok()
            .flatten()
            .map(|values| {
                serde_json::Value::Array(
                    values
                        .into_iter()
                        .map(|value| serde_json::json!(value))
                        .collect(),
                )
            })
            .or_else(|| {
                row.try_get::<_, Option<PostgresArrayText>>(name)
                    .ok()
                    .flatten()
                    .and_then(|value| {
                        parse_postgres_text_array(&value.0, |item| {
                            item.parse::<f32>()
                                .ok()
                                .map(|parsed| serde_json::json!(parsed))
                        })
                    })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::FLOAT8_ARRAY => row
            .try_get::<_, Option<Vec<f64>>>(name)
            .ok()
            .flatten()
            .map(|values| {
                serde_json::Value::Array(
                    values
                        .into_iter()
                        .map(|value| serde_json::json!(value))
                        .collect(),
                )
            })
            .or_else(|| {
                row.try_get::<_, Option<PostgresArrayText>>(name)
                    .ok()
                    .flatten()
                    .and_then(|value| {
                        parse_postgres_text_array(&value.0, |item| {
                            item.parse::<f64>()
                                .ok()
                                .map(|parsed| serde_json::json!(parsed))
                        })
                    })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::TEXT_ARRAY | postgres::types::Type::VARCHAR_ARRAY => row
            .try_get::<_, Option<Vec<String>>>(name)
            .ok()
            .flatten()
            .map(|values| {
                serde_json::Value::Array(
                    values.into_iter().map(serde_json::Value::String).collect(),
                )
            })
            .or_else(|| {
                row.try_get::<_, Option<PostgresArrayText>>(name)
                    .ok()
                    .flatten()
                    .and_then(|value| {
                        parse_postgres_text_array(&value.0, |item| {
                            Some(serde_json::Value::String(item.to_string()))
                        })
                    })
            })
            .unwrap_or(serde_json::Value::Null),
        postgres::types::Type::NUMERIC_ARRAY => row
            .try_get::<_, Option<PostgresArrayText>>(name)
            .ok()
            .flatten()
            .and_then(|value| {
                parse_postgres_text_array(&value.0, |item| {
                    Some(serde_json::Value::String(item.to_string()))
                })
            })
            .unwrap_or(serde_json::Value::Null),
        _ => string_value()
            .map(serde_json::Value::String)
            .unwrap_or(serde_json::Value::Null),
    }
}

fn postgres_fields_json(fields: &[PostgresFieldMetadata]) -> serde_json::Value {
    serde_json::Value::Array(
        fields
            .iter()
            .map(|field| {
                serde_json::json!({
                    "name": field.name,
                    "dataTypeID": field.data_type_id,
                    "format": field.format,
                })
            })
            .collect(),
    )
}

fn parse_postgres_bool_array_element(item: &str) -> Option<serde_json::Value> {
    match item {
        "t" | "true" => Some(serde_json::Value::Bool(true)),
        "f" | "false" => Some(serde_json::Value::Bool(false)),
        _ => None,
    }
}

fn parse_postgres_text_array<F>(raw: &str, parse_item: F) -> Option<serde_json::Value>
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

    let push_current =
        |values: &mut Vec<serde_json::Value>, current: &mut String, quoted_current: &mut bool| {
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
    use super::{
        parse_postgres_bool_array_element, parse_postgres_text_array, validate_outbound_host,
    };

    #[test]
    fn parses_text_arrays() {
        let parsed = parse_postgres_text_array("{alpha,beta}", |item| {
            Some(serde_json::Value::String(item.to_string()))
        });

        assert_eq!(parsed, Some(serde_json::json!(["alpha", "beta"])));
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
        let parsed =
            parse_postgres_text_array("{t,f,true,false}", parse_postgres_bool_array_element);

        assert_eq!(parsed, Some(serde_json::json!([true, false, true, false])));
    }

    #[test]
    fn postgres_allow_env_allows_private_hosts() {
        unsafe {
            std::env::set_var("FLUXBASE_ALLOW_LOOPBACK_POSTGRES", "1");
        }

        let result = validate_outbound_host(
            "172.18.0.2",
            5432,
            "postgres connect",
            "FLUXBASE_ALLOW_LOOPBACK_POSTGRES",
        );

        unsafe {
            std::env::remove_var("FLUXBASE_ALLOW_LOOPBACK_POSTGRES");
        }

        assert!(result.is_ok());
    }
}

fn decode_checkpoint_bytes(value: &serde_json::Value, field: &str) -> Result<Vec<u8>, JsErrorBox> {
    let encoded = value
        .get(field)
        .and_then(|value| value.as_str())
        .ok_or_else(|| {
            JsErrorBox::type_error(format!("recorded tcp checkpoint missing {field}"))
        })?;
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
    let connect_timeout =
        Duration::from_millis(connect_timeout_ms.unwrap_or(DEFAULT_CONNECT_TIMEOUT_MS));
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
            return Err(JsErrorBox::type_error(
                "invalid CA certificate PEM: no certificates found",
            ));
        }

        for cert in certs {
            roots.add(cert).map_err(|err| {
                JsErrorBox::type_error(format!("invalid CA certificate PEM: {err}"))
            })?;
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
            let expected = read_bytes
                .ok_or_else(|| JsErrorBox::type_error("tcp fixed read mode requires readBytes"))?;
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
            let expected = read_bytes
                .ok_or_else(|| JsErrorBox::type_error("tcp fixed read mode requires readBytes"))?;
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

/// Returns deterministic monotonic high-resolution time since execution start.
/// This acts as an IO boundary so repeated calls advance the timer predictably.
#[op2(fast)]
fn op_flux_now_high_res(state: &mut OpState, #[string] execution_id: String) -> f64 {
    let now_hr = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as f64 / 1000.0;
        
    let map = state.borrow_mut::<RuntimeStateMap>();
    let exec = match map.get_mut(&execution_id) {
        Some(e) => e,
        None => return now_hr,
    };
    
    let call_index = exec.call_index;
    exec.call_index = exec.call_index.saturating_add(1);

    match exec.context.mode {
        ExecutionMode::Live => {
            // Rebase relative to the execution start time if possible
            let start = exec.recorded_now_ms.unwrap_or(0) as f64;
            let elapsed = if start > 0.0 { (now_hr - start).max(0.0) } else { 0.0 };
            
            exec.checkpoints.push(FetchCheckpoint {
                call_index,
                boundary: "performance.now".to_string(),
                url: "".to_string(),
                method: "".to_string(),
                request: serde_json::json!({}),
                response: serde_json::json!(elapsed),
                duration_ms: 0,
            });
            elapsed
        }
        ExecutionMode::Replay => {
            let checkpoint = exec.recorded.remove(&call_index).unwrap_or_else(|| {
                FetchCheckpoint {
                    call_index,
                    boundary: "performance.now".to_string(),
                    url: "".to_string(),
                    method: "".to_string(),
                    request: serde_json::json!({}),
                    response: serde_json::json!(0.0),
                    duration_ms: 0,
                }
            });
            checkpoint.response.as_f64().unwrap_or(0.0)
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
            level: if is_err {
                "error".to_string()
            } else {
                "log".to_string()
            },
            message: msg.clone(),
        });
    }
    let verbose = if let Some(exec) = map.get(&execution_id) {
        exec.context.verbose
    } else {
        false
    };

    if verbose {
        if is_err {
            eprintln!("{msg}");
        } else {
            println!("{msg}");
        }
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
                    if let Some(cp) = &recorded {
                        print_checkpoint_replay(cp);
                    }
                    let effective_delay_ms = recorded
                        .as_ref()
                        .and_then(|checkpoint| checkpoint.response.get("effective_delay_ms"))
                        .and_then(|value| value.as_f64())
                        .unwrap_or_else(|| {
                            if delay_ms.is_sign_negative() {
                                0.0
                            } else {
                                delay_ms
                            }
                        });

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
                    let effective_delay_ms = if delay_ms.is_sign_negative() {
                        0.0
                    } else {
                        delay_ms
                    };
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

#[op2]
#[serde]
fn op_flux_crypto_replay(
    state: &mut OpState,
    #[string] execution_id: String,
) -> Result<serde_json::Value, JsErrorBox> {
    let map = state.borrow_mut::<RuntimeStateMap>();
    let execution = map.get_mut(&execution_id).ok_or_else(|| {
        JsErrorBox::new(
            "Error",
            format!("op_flux_crypto_replay: execution_id '{execution_id}' not found"),
        )
    })?;

    let call_index = execution.call_index;
    execution.call_index = execution.call_index.saturating_add(1);

    if matches!(execution.context.mode, ExecutionMode::Replay) {
        if let Some(checkpoint) = execution.recorded.remove(&call_index) {
            return Ok(serde_json::json!({
                "call_index": call_index,
                "has_recorded": true,
                "response": checkpoint.response
            }));
        }
    }

    Ok(serde_json::json!({
        "call_index": call_index,
        "has_recorded": false,
        "response": serde_json::Value::Null
    }))
}

#[op2]
fn op_flux_crypto_record(
    state: &mut OpState,
    #[string] execution_id: String,
    call_index: u32,
    #[string] boundary: String,
    #[serde] request: serde_json::Value,
    #[serde] response: serde_json::Value,
) -> Result<(), JsErrorBox> {
    let map = state.borrow_mut::<RuntimeStateMap>();
    if let Some(execution) = map.get_mut(&execution_id) {
        execution.checkpoints.push(FetchCheckpoint {
            call_index,
            boundary,
            url: "".to_string(),
            method: "".to_string(),
            request,
            response,
            duration_ms: 0,
        });
    }
    Ok(())
}

#[op2]
#[serde]
fn op_random_bytes(state: &mut OpState, #[string] execution_id: String, #[smi] len: u32) -> Result<serde_json::Value, JsErrorBox> {
    let map = state.borrow_mut::<RuntimeStateMap>();
    let exec = map.get_mut(&execution_id).ok_or_else(|| {
        JsErrorBox::new(
            "Error",
            format!("op_random_bytes: execution_id '{execution_id}' not found"),
        )
    })?;
    
    let call_index = exec.call_index;
    exec.call_index = exec.call_index.saturating_add(1);

    match exec.context.mode {
        ExecutionMode::Live => {
            let mut bytes = vec![0u8; len as usize];
            rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut bytes);
            let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
            
            exec.checkpoints.push(FetchCheckpoint {
                call_index,
                boundary: "crypto.getRandomValues".to_string(),
                url: "".to_string(),
                method: "".to_string(),
                request: serde_json::json!({ "length": len }),
                response: serde_json::json!({ "bytes": b64 }),
                duration_ms: 0,
            });
            
            Ok(serde_json::json!({
                "bytes": b64
            }))
        }
        ExecutionMode::Replay => {
            let checkpoint = exec.recorded.remove(&call_index).unwrap_or_else(|| {
                FetchCheckpoint {
                    call_index,
                    boundary: "crypto.getRandomValues".to_string(),
                    url: "".to_string(),
                    method: "".to_string(),
                    request: serde_json::json!({}),
                    response: serde_json::json!({ "bytes": "" }),
                    duration_ms: 0,
                }
            });
            Ok(checkpoint.response)
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
            let val = exec
                .recorded_uuids
                .get(idx)
                .cloned()
                .unwrap_or_else(|| Uuid::new_v4().to_string());
            println!("  \x1b[2m›\x1b[0m \x1b[1mUUID\x1b[0m  \x1b[2m→\x1b[0m {}", val);
            val
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
        exec.pending_responses.insert(
            req_id,
            NetResponse {
                status: status as u16,
                headers,
                body,
            },
        );
    }
}

fn make_http_request(
    url: &str,
    method: &str,
    body: Option<String>,
    headers: Option<HashMap<String, String>>,
) -> Result<serde_json::Value, JsErrorBox> {
    let agent = ureq::builder().redirects(0).build();
    let request_headers: BTreeMap<String, String> = headers
        .unwrap_or_default()
        .into_iter()
        .map(|(key, value)| (key.to_ascii_lowercase(), value))
        .collect();
    let method = method.to_ascii_uppercase();

    if let Some(response) = cached_http_response(&method, url, &request_headers) {
        return Ok(response);
    }

    let mut current_url = url.to_string();
    let mut current_method = method.clone();
    let mut current_body = body;
    let response = {
        let mut final_response = None;
        for redirect_count in 0..=MAX_REDIRECTS {
            validate_fetch_url(&current_url)?;

            let mut request = agent.request(&current_method, &current_url);
            for (key, value) in &request_headers {
                request = request.set(key, value);
            }

            let current_body_bytes = current_body.as_ref().map(|b| {
                BASE64_STANDARD.decode(b).unwrap_or_else(|_| b.clone().into_bytes())
            });

            let response = match current_body_bytes.as_ref() {
                Some(body_bytes) => request.send_bytes(body_bytes),
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
                        .map_err(|err| {
                            JsErrorBox::type_error(format!("invalid redirect URL: {err}"))
                        })?
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
            return Err(JsErrorBox::type_error(format!(
                "response too large: {len} bytes exceeds {MAX_RESPONSE_BYTES} byte limit"
            )));
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
        return Err(JsErrorBox::type_error(format!(
            "response body too large: {} bytes exceeds {MAX_RESPONSE_BYTES} byte limit",
            bytes.len()
        )));
    }

    let mut response_json = serde_json::json!({
        "status": status,
        "headers": response_headers,
    });

    if let Ok(text) = String::from_utf8(bytes.clone()) {
        let parsed_body = serde_json::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|_| serde_json::Value::String(text));
        response_json["body"] = parsed_body;
    } else {
        let b64 = BASE64_STANDARD.encode(&bytes);
        response_json["body"] = serde_json::Value::String(b64);
        response_json["is_binary"] = serde_json::Value::Bool(true);
    }

    store_http_response_cache(
        &method,
        url,
        &request_headers,
        &response_headers,
        &response_json,
        status,
    );

    Ok(response_json)
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

fn canonicalize_existing_path(path: &Path) -> Result<PathBuf> {
    if !path.exists() {
        anyhow::bail!("entry file not found: {}", path.display());
    }
    path.canonicalize()
        .with_context(|| format!("failed to resolve {}", path.display()))
}

fn resolve_existing_module_path(path: &Path) -> Result<PathBuf> {
    if path.is_file() {
        return canonicalize_existing_path(path);
    }

    if path.extension().is_none() {
        for extension in MODULE_FILE_EXTENSIONS {
            let candidate = path.with_extension(extension);
            if candidate.is_file() {
                return canonicalize_existing_path(&candidate);
            }
        }
    }

    if path.is_dir() {
        if let Some(package_entry) = resolve_runtime_package_entry(path)? {
            return Ok(package_entry);
        }
    }

    if path.extension().is_none() {
        for extension in MODULE_FILE_EXTENSIONS {
            let candidate = path.join(format!("index.{extension}"));
            if candidate.is_file() {
                return canonicalize_existing_path(&candidate);
            }
        }
    }

    anyhow::bail!("entry file not found: {}", path.display())
}

fn read_runtime_package_manifest(path: &Path) -> Result<RuntimePackageManifest> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&source).with_context(|| format!("failed to parse {}", path.display()))
}

fn resolve_runtime_package_entry(package_dir: &Path) -> Result<Option<PathBuf>> {
    let manifest_path = package_dir.join("package.json");
    if !manifest_path.is_file() {
        return Ok(None);
    }

    let manifest = read_runtime_package_manifest(&manifest_path)?;
    for candidate in [manifest.module.as_deref(), manifest.main.as_deref()]
        .into_iter()
        .flatten()
    {
        let resolved = resolve_existing_module_path(&package_dir.join(candidate))?;
        return Ok(Some(resolved));
    }

    for extension in MODULE_FILE_EXTENSIONS {
        let candidate = package_dir.join(format!("index.{extension}"));
        if candidate.is_file() {
            return Ok(Some(canonicalize_existing_path(&candidate)?));
        }
    }

    Ok(None)
}

fn runtime_bare_package_parts(specifier: &str) -> Option<(String, Option<String>)> {
    if specifier.is_empty()
        || specifier.starts_with("./")
        || specifier.starts_with("../")
        || specifier.starts_with('/')
        || specifier.starts_with("file://")
        || specifier.starts_with("https://")
        || specifier.starts_with("http://")
        || specifier.starts_with("npm:")
        || specifier.starts_with("node:")
    {
        return None;
    }

    if let Some(stripped) = specifier.strip_prefix('@') {
        let mut parts = stripped.split('/');
        let scope = parts.next()?;
        let name = parts.next()?;
        let package_name = format!("@{scope}/{name}");
        let remainder = parts.collect::<Vec<_>>().join("/");
        return if remainder.is_empty() {
            Some((package_name, None))
        } else {
            Some((package_name, Some(remainder)))
        };
    }

    let mut parts = specifier.split('/');
    let package_name = parts.next()?.to_string();
    let remainder = parts.collect::<Vec<_>>().join("/");
    if remainder.is_empty() {
        Some((package_name, None))
    } else {
        Some((package_name, Some(remainder)))
    }
}

fn resolve_local_node_module_specifier(
    specifier: &str,
    referrer: &str,
) -> Result<Option<ModuleSpecifier>> {
    let (package_name, subpath) = match runtime_bare_package_parts(specifier) {
        Some(parts) => parts,
        None => return Ok(None),
    };

    let referrer_url =
        Url::parse(referrer).with_context(|| format!("invalid referrer: {referrer}"))?;
    if referrer_url.scheme() != "file" {
        return Ok(None);
    }

    let referrer_path = referrer_url
        .to_file_path()
        .map_err(|_| anyhow::anyhow!("invalid file referrer: {referrer}"))?;

    for ancestor in referrer_path.ancestors().skip(1) {
        let package_dir = ancestor.join("node_modules").join(&package_name);
        if !package_dir.exists() {
            continue;
        }

        let target = if let Some(subpath) = subpath.as_deref() {
            resolve_existing_module_path(&package_dir.join(subpath))?
        } else if let Some(package_entry) = resolve_runtime_package_entry(&package_dir)? {
            package_entry
        } else {
            resolve_existing_module_path(&package_dir)?
        };

        return Url::from_file_path(&target)
            .map_err(|_| anyhow::anyhow!("failed to convert {} to file URL", target.display()))
            .map(Some);
    }

    Ok(None)
}

impl ModuleLoader for TypescriptModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: ResolutionKind,
    ) -> std::result::Result<ModuleSpecifier, deno_core::error::ModuleLoaderError> {
        if specifier == "pg" {
            return Url::parse(FLUX_PG_SPECIFIER)
                .map_err(JsErrorBox::from_err)
                .map_err(Into::into);
        }
        if specifier == "redis" {
            return Url::parse(FLUX_REDIS_SPECIFIER)
                .map_err(JsErrorBox::from_err)
                .map_err(Into::into);
        }

        if let Some(local_specifier) = resolve_local_node_module_specifier(specifier, referrer)
            .map_err(|err| JsErrorBox::generic(err.to_string()))?
        {
            return Ok(local_specifier);
        }

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
            _options: &ModuleLoadOptions,
        ) -> std::result::Result<ModuleSource, deno_core::error::ModuleLoaderError> {
            if module_specifier.as_str() == FLUX_PG_SPECIFIER {
                return Ok(ModuleSource::new(
                    ModuleType::JavaScript,
                    ModuleSourceCode::String(flux_pg_module_js().to_string().into()),
                    module_specifier,
                    None,
                ));
            }
            if module_specifier.as_str() == FLUX_REDIS_SPECIFIER {
                return Ok(ModuleSource::new(
                    ModuleType::JavaScript,
                    ModuleSourceCode::String(flux_redis_module_js().to_string().into()),
                    module_specifier,
                    None,
                ));
            }

            let path = module_specifier
                .to_file_path()
                .map_err(|_| JsErrorBox::generic(format!("Only file:// URLs are supported: {}", module_specifier)))?;

            let media_type = MediaType::from_path(&path);
            let module_type = match media_type {
                MediaType::JavaScript | MediaType::Mjs | MediaType::Cjs => ModuleType::JavaScript,
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

            let source = std::fs::read_to_string(&path).map_err(JsErrorBox::from_err)?;
            let source =
                transpile_module_source(module_specifier, media_type, source, Some(&source_maps))?;

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
        if specifier == "pg" {
            return Url::parse(FLUX_PG_SPECIFIER)
                .map_err(JsErrorBox::from_err)
                .map_err(Into::into);
        }
        if specifier == "redis" || specifier == FLUX_REDIS_SPECIFIER {
            return Url::parse(FLUX_REDIS_SPECIFIER)
                .map_err(JsErrorBox::from_err)
                .map_err(Into::into);
        }
        if specifier == "pg" || specifier == "postgres" || specifier == FLUX_PG_SPECIFIER {
            return Url::parse(FLUX_PG_SPECIFIER)
                .map_err(JsErrorBox::from_err)
                .map_err(Into::into);
        }
        if specifier == "node:http" || specifier == "http" || specifier.contains("/node/http.mjs") {
            return Url::parse(FLUX_HTTP_SPECIFIER)
                .map_err(JsErrorBox::from_err)
                .map_err(Into::into);
        }
        if specifier.starts_with("flux:") {
            return Url::parse(specifier)
                .map_err(JsErrorBox::from_err)
                .map_err(Into::into);
        }
        if let Some(module) = self.modules.get(referrer) {
            if let Some(dependency) = module
                .dependencies
                .iter()
                .find(|dependency| dependency.specifier == specifier)
            {
                let resolved =
                    Url::parse(&dependency.resolved_specifier).map_err(JsErrorBox::from_err)?;
                if self.modules.contains_key(resolved.as_str()) {
                    return Ok(resolved);
                }
            }
        }

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
            if module_specifier.as_str() == FLUX_PG_SPECIFIER {
                return Ok(ModuleSource::new(
                    ModuleType::JavaScript,
                    ModuleSourceCode::String(flux_pg_module_js().to_string().into()),
                    module_specifier,
                    None,
                ));
            }
            if module_specifier.as_str() == FLUX_REDIS_SPECIFIER {
                return Ok(ModuleSource::new(
                    ModuleType::JavaScript,
                    ModuleSourceCode::String(flux_redis_module_js().to_string().into()),
                    module_specifier,
                    None,
                ));
            }
            if module_specifier.as_str() == FLUX_HTTP_SPECIFIER {
                return Ok(ModuleSource::new(
                    ModuleType::JavaScript,
                    ModuleSourceCode::String(flux_http_module_js().to_string().into()),
                    module_specifier,
                    None,
                ));
            }

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

        ModuleLoadResponse::Sync(load_module(
            source_maps,
            modules,
            &module_specifier,
            &options,
        ))
    }

    fn get_source_map(&self, specifier: &str) -> Option<Cow<'_, [u8]>> {
        self.source_maps
            .borrow()
            .get(specifier)
            .map(|value| value.clone().into())
    }
}

fn flux_http_module_js() -> &'static str {
    r#"
export class Server {
  listen() { return this; }
  on() { return this; }
  once() { return this; }
  emit() { return true; }
  close() { return this; }
  address() { return { port: 0, family: 'IPv4', address: '127.0.0.1' }; }
  ref() { return this; }
  unref() { return this; }
}
export function createServer() { return new Server(); }
export default { Server, createServer };
"#
}

fn flux_pg_module_js() -> &'static str {
    r#"
const __fluxPg = globalThis.Flux?.postgres;

if (!__fluxPg || !__fluxPg.NodePgPool || !__fluxPg.nodePgTypes) {
    throw new Error("Flux pg shim is unavailable");
}

function __fluxPgNormalizeConfig(config = {}) {
    const connectionString = String(config?.connectionString ?? "");
    const ssl = config?.ssl;
    
    // Automatically enable TLS if requested in the connection string
    const urlHasSsl = connectionString.includes("sslmode=require") || 
                     connectionString.includes("sslmode=prefer") ||
                     connectionString.includes("sslmode=allow");

    return {
        connectionString,
        tls: ssl !== false && (!!ssl || urlHasSsl),
        caCertPem: ssl && typeof ssl === "object" && ssl.ca != null ? String(ssl.ca) : null,
    };
}

class DatabaseError extends Error {
    constructor(message, details = {}) {
        super(message);
        this.name = "DatabaseError";
        Object.assign(this, details);
    }
}

function __fluxPgWrapDatabaseError(error) {
    if (error instanceof DatabaseError) {
        return error;
    }
    if (error && typeof error === "object" && (error.name === "DatabaseError" || error.code != null)) {
        return new DatabaseError(String(error.message ?? "postgres query failed"), error);
    }
    return error;
}

async function __fluxPgWrapQueryError(runQuery) {
    try {
        return await runQuery();
    } catch (error) {
        throw __fluxPgWrapDatabaseError(error);
    }
}

class Client {
    constructor(config = {}) {
        this._config = __fluxPgNormalizeConfig(config);
        this._inner = null;
        this._released = false;
    }

    static __fromInner(inner, config = {}) {
        const client = new Client(config);
        client._inner = inner;
        return client;
    }

    async connect() {
        if (this._released) {
            throw new Error("pg Client has already been closed");
        }
        if (this._inner) {
            return this;
        }
        const pool = new __fluxPg.NodePgPool(this._config);
        this._pool = pool;
        this._inner = await pool.connect();
        return this;
    }

    async query(queryOrConfig, values = undefined) {
        if (!this._inner) {
            await this.connect();
        }
        return __fluxPgWrapQueryError(() => this._inner.query(queryOrConfig, values));
    }

    async release() {
        if (this._released) {
            return undefined;
        }
        this._released = true;
        if (this._inner) {
            await this._inner.release();
            this._inner = null;
        }
        if (this._pool) {
            await this._pool.end();
            this._pool = null;
        }
        return undefined;
    }

    async end() {
        return this.release();
    }
}

class Pool {
    constructor(config = {}) {
        this.options = { ...config };
        this._config = __fluxPgNormalizeConfig(config);
        this._inner = new __fluxPg.NodePgPool(this._config);
    }

    async query(queryOrConfig, values = undefined) {
        return __fluxPgWrapQueryError(() => this._inner.query(queryOrConfig, values));
    }

    async connect() {
        const inner = await this._inner.connect();
        return Client.__fromInner(inner, this.options);
    }

    async end() {
        return this._inner.end();
    }
}

const types = __fluxPg.nodePgTypes;
const defaults = {};
const native = null;

export { Client, DatabaseError, Pool, defaults, native, types };
export default { Client, DatabaseError, Pool, defaults, native, types };
"#
}

fn flux_redis_module_js() -> &'static str {
    r#"
const __fluxRedis = globalThis.Flux?.redis;

if (!__fluxRedis || typeof __fluxRedis.createClient !== "function") {
    throw new Error("Flux redis shim is unavailable");
}

function createClient(options = {}) {
    return __fluxRedis.createClient(options);
}

export { createClient };
export default { createClient };
"#
}

pub struct JsIsolate {
    runtime: JsRuntime,
    /// True when the user module called `Deno.serve()` during module init,
    /// meaning the isolate acts as a long-running HTTP app, not a one-shot handler.
    pub is_server_mode: bool,
}

impl JsIsolate {
    pub async fn new(user_code: &str, _isolate_id: usize) -> Result<Self> {
        Self::new_internal(user_code, prepare_user_code(user_code)).await
    }

    /// Variant used by `flux run` / `--script-mode`.  Accepts plain top-level
    /// scripts (no `export default` required) while still wiring up the handler
    /// global when `export default` IS present.
    pub async fn new_for_run(user_code: &str) -> Result<Self> {
        Self::new_internal(user_code, prepare_run_code(user_code)).await
    }

    /// Variant used by `flux run` when loading a real JS/TS module entry.
    /// Supports relative ESM imports and TypeScript transpilation on demand.
    pub async fn new_for_run_entry(entry: &Path) -> Result<Self> {
        let source_maps = Rc::new(RefCell::new(HashMap::new()));
        let mut runtime = JsRuntime::new(RuntimeOptions {
            module_loader: Some(Rc::new(TypescriptModuleLoader { 
                source_maps,
            })),
            extensions: flux_extensions(),
            create_params: Some(
                deno_core::v8::CreateParams::default().heap_limits(0, V8_HEAP_LIMIT),
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

        runtime
            .execute_script("flux:bootstrap", bootstrap_js())
            .context("failed to install bootstrap globals")?;

        let main_module = resolve_path(
            entry
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("invalid entry path: {}", entry.display()))?,
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
        .map_err(|_| {
            anyhow::anyhow!("module initialization timed out after {EXECUTION_TIMEOUT:?}")
        })?
        .context("event loop error during module initialization")?;
        evaluation.await.context("failed to evaluate user module")?;

        close_registration_phase(&mut runtime)?;

        let is_server_mode = { probe_server_mode(&mut runtime)? };

        Ok(Self {
            runtime,
            is_server_mode,
        })
    }

    pub async fn new_from_any_artifact(artifact: &RuntimeArtifact, isolate_id: usize) -> Result<Self> {
        match artifact {
            RuntimeArtifact::Built(b) => Self::new_from_artifact(b).await,
            RuntimeArtifact::Inline(i) => Self::new(&i.code, isolate_id).await,
        }
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
            extensions: flux_extensions(),
            create_params: Some(
                deno_core::v8::CreateParams::default().heap_limits(0, V8_HEAP_LIMIT),
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

        runtime
            .execute_script("flux:bootstrap", bootstrap_js())
            .context("failed to install bootstrap globals")?;

        let entry_module = artifact
            .modules
            .iter()
            .find(|module| module.specifier == artifact.entry_specifier)
            .ok_or_else(|| anyhow::anyhow!("entry module missing from built artifact"))?;
        let main_module = Url::parse(&artifact.entry_specifier).with_context(|| {
            format!(
                "invalid entry module specifier: {}",
                artifact.entry_specifier
            )
        })?;
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
        .map_err(|_| {
            anyhow::anyhow!("module initialization timed out after {EXECUTION_TIMEOUT:?}")
        })?
        .context("event loop error during artifact module initialization")?;
        evaluation
            .await
            .context("failed to evaluate built artifact entry module")?;

        close_registration_phase(&mut runtime)?;

        let is_server_mode = { probe_server_mode(&mut runtime)? };

        Ok(Self {
            runtime,
            is_server_mode,
        })
    }

    async fn new_internal(_user_code: &str, prepared: String) -> Result<Self> {
        let source_maps = Rc::new(RefCell::new(HashMap::new()));
        let mut runtime = JsRuntime::new(RuntimeOptions {
            module_loader: Some(Rc::new(TypescriptModuleLoader {
                source_maps,
            })),
            extensions: flux_extensions(),
            create_params: Some(
                deno_core::v8::CreateParams::default().heap_limits(0, V8_HEAP_LIMIT),
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
            .execute_script("flux:bootstrap", bootstrap_js())
            .context("failed to install bootstrap globals")?;

        let specifier = Url::parse("file:///flux_user_code.ts").unwrap();
        let module_id = runtime
            .load_main_es_module_from_code(&specifier, prepared)
            .await
            .context("failed to load user code as module")?;
        
        let evaluation = runtime.mod_evaluate(module_id);
        tokio::time::timeout(
            EXECUTION_TIMEOUT,
            runtime.run_event_loop(Default::default()),
        )
        .await
        .map_err(|_| {
            anyhow::anyhow!("module initialization timed out after {EXECUTION_TIMEOUT:?}")
        })?
        .context("event loop error during module initialization")?;
        evaluation.await.context("failed to evaluate user module")?;

        close_registration_phase(&mut runtime)?;

        // Check if the module called Deno.serve() during init.
        // In the new model, server-mode detection uses a bootstrap execution slot.
        let is_server_mode = {
            let state = runtime.op_state();
            let state = state.borrow();
            drop(state);
            probe_server_mode(&mut runtime)?
        };

        Ok(Self {
            runtime,
            is_server_mode,
        })
    }

    /// Dispatch a single HTTP request into a server-mode isolate.  The JS
    /// `__flux_dispatch_request` shim feeds the request through the registered
    /// Hono / Express handler, which calls `op_net_respond` when done.
    pub async fn dispatch_request(
        &mut self,
        context: ExecutionContext,
        req: NetRequest,
    ) -> Result<NetRequestExecution> {
        self.dispatch_request_with_recorded(context, req, Vec::new())
            .await
    }

    pub async fn dispatch_request_with_recorded(
        &mut self,
        context: ExecutionContext,
        req: NetRequest,
        recorded_checkpoints: Vec<FetchCheckpoint>,
    ) -> Result<NetRequestExecution> {
        unsafe { std::env::set_var("DATABASE_URL", std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://localhost/test".to_string())); }
        let execution_id = context.execution_id.clone();
        let request_id = context.request_id.clone();
        let recorded: HashMap<u32, FetchCheckpoint> = recorded_checkpoints
            .into_iter()
            .map(|cp| (cp.call_index, cp))
            .collect();

        // Register a state slot for this request.
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let map = state.borrow_mut::<RuntimeStateMap>();
            map.insert(
                execution_id.clone(),
                RuntimeExecutionState {
                    context,
                    call_index: 0,
                    checkpoints: Vec::new(),
                    recorded,
                    recorded_now_ms: None,
                    logs: Vec::new(),
                    has_live_io: false,
                    boundary_stop: None,
                    recorded_random: Vec::new(),
                    random_index: 0,
                    recorded_uuids: Vec::new(),
                    uuid_index: 0,
                    is_server_mode: true,
                    pending_responses: HashMap::new(),
                    postgres_sessions: HashMap::new(),
                    next_postgres_session_id: 0,
                },
            );
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

        let error_val = self
            .runtime
            .execute_script(
                "flux:get_last_error",
                format!("JSON.stringify(globalThis.__flux_last_error || {{}})")
            )?;
        
        let error_json: String = {
            deno_core::scope!(scope, &mut self.runtime);
            let local = deno_core::v8::Local::new(scope, error_val);
            if let Some(s) = local.to_string(scope) {
                s.to_rust_string_lossy(scope)
            } else {
                "{}".to_string()
            }
        };
        
        let error: Option<String> = serde_json::from_str::<serde_json::Value>(&error_json)
            .ok()
            .and_then(|v| v.get(&execution_id).cloned())
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        let state = self.runtime.op_state();
        let mut state = state.borrow_mut();
        let map = state.borrow_mut::<RuntimeStateMap>();
        let exec = map
            .remove(&execution_id)
            .ok_or_else(|| anyhow::anyhow!("state slot missing for execution {execution_id}"))?;

        // If replay stopped at a boundary (no recorded checkpoint), return the
        // boundary stop as a first-class outcome rather than an error.
        if let Some(boundary) = exec.boundary_stop.clone() {
            let sentinel_error = format!("__FLUX_BOUNDARY_STOP:{boundary}");
            let dummy_response = NetResponse {
                status: 200,
                headers: Vec::new(),
                body: String::new(),
            };
            return Ok(NetRequestExecution {
                response: dummy_response,
                checkpoints: exec.checkpoints,
                error: Some(sentinel_error),
                logs: exec.logs,
                has_live_io: false,
                boundary_stop: Some(boundary),
            });
        }

        let response = exec.pending_responses.into_values().next().ok_or_else(|| {
            anyhow::anyhow!(
                "handler did not call op_net_respond for req {} (request_id={})",
                req.req_id,
                request_id
            )
        })?;

        Ok(NetRequestExecution {
            response,
            checkpoints: exec.checkpoints,
            error,
            logs: exec.logs,
            has_live_io: exec.has_live_io,
            boundary_stop: exec.boundary_stop,
        })
    }

    pub async fn execute(
        &mut self,
        payload: serde_json::Value,
        context: ExecutionContext,
    ) -> Result<JsExecutionOutput> {
        self.execute_with_recorded(payload, context, Vec::new())
            .await
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
        self.execute_handler_with_recorded(payload, context, recorded_checkpoints, true)
            .await
    }

    pub fn has_handler(&mut self) -> Result<bool> {
        let check = self
            .runtime
            .execute_script(
                "flux:check_handler",
                "typeof globalThis.__flux_user_handler === 'function'",
            )
            .context("failed to check for exported handler")?;
        deno_core::scope!(scope, &mut self.runtime);
        let local = deno_core::v8::Local::new(scope, check);
        Ok(local.is_true())
    }

    pub async fn execute_handler(
        &mut self,
        input: serde_json::Value,
        context: ExecutionContext,
    ) -> Result<JsExecutionOutput> {
        self.execute_handler_with_recorded(input, context, Vec::new(), true)
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
            map.insert(
                execution_id.clone(),
                RuntimeExecutionState {
                    context,
                    call_index: 0,
                    checkpoints: Vec::new(),
                    recorded,
                    recorded_now_ms: None,
                    logs: Vec::new(),
                    has_live_io: false,
                    boundary_stop: None,
                    recorded_random: Vec::new(),
                    random_index: 0,
                    recorded_uuids: Vec::new(),
                    uuid_index: 0,
                    is_server_mode: false,
                    pending_responses: HashMap::new(),
                    postgres_sessions: HashMap::new(),
                    next_postgres_session_id: 0,
                },
            );
        }

        let eid_json =
            serde_json::to_string(&execution_id).context("failed to encode execution_id")?;
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

        let envelope: serde_json::Value =
            serde_json::from_str(&raw).context("handler result envelope is not valid JSON")?;

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

        let output = envelope
            .get("result")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let state = self.runtime.op_state();
        let mut state = state.borrow_mut();
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.remove(&execution_id);
        let has_live_io = execution.as_ref().map(|e| e.has_live_io).unwrap_or(false);
        let boundary_stop = execution.as_ref().and_then(|e| e.boundary_stop.clone());
        Ok(JsExecutionOutput {
            output,
            checkpoints,
            error,
            logs,
            has_live_io,
            boundary_stop,
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
    pub async fn run_script(
        &mut self,
        input: serde_json::Value,
        context: ExecutionContext,
    ) -> Result<JsExecutionOutput> {
        let started = std::time::Instant::now();
        let execution_id = context.execution_id.clone();

        emit_flux_event(serde_json::json!({
            "type": "execution_start",
            "id": execution_id,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        }));

        if self.has_handler()? {
            let res = self
                .execute_handler_with_recorded(input, context, Vec::new(), false)
                .await;
            
            let duration_ms = started.elapsed().as_millis() as u64;
            let status = if res.as_ref().map(|o| o.error.is_none()).unwrap_or(false) { "ok" } else { "error" };
            
            emit_flux_event(serde_json::json!({
                "type": "execution_end",
                "id": execution_id,
                "status": status,
                "duration_ms": duration_ms,
            }));

            return res;
        }

        // Top-level mode: register a transient state slot so ops don't panic,
        // then drain the event loop.
        let execution_id = context.execution_id.clone();
        {
            let state = self.runtime.op_state();
            let mut state = state.borrow_mut();
            let map = state.borrow_mut::<RuntimeStateMap>();
            map.insert(
                execution_id.clone(),
                RuntimeExecutionState {
                    context,
                    call_index: 0,
                    checkpoints: Vec::new(),
                    recorded: HashMap::new(),
                    recorded_now_ms: None,
                    logs: Vec::new(),
                    has_live_io: false,
                    boundary_stop: None,
                    recorded_random: Vec::new(),
                    random_index: 0,
                    recorded_uuids: Vec::new(),
                    uuid_index: 0,
                    is_server_mode: false,
                    pending_responses: HashMap::new(),
                    postgres_sessions: HashMap::new(),
                    next_postgres_session_id: 0,
                },
            );
        }

        // Tell bootstrap JS which execution_id to use for top-level ops.
        let eid_json = serde_json::to_string(&execution_id).unwrap();
        self.runtime
            .execute_script(
                "flux:set_script_eid",
                format!("globalThis.__FLUX_EXECUTION_ID__ = {eid_json};"),
            )
            .context("failed to set execution_id")?;

        tokio::time::timeout(
            EXECUTION_TIMEOUT,
            self.runtime.run_event_loop(Default::default()),
        )
        .await
        .map_err(|_| anyhow::anyhow!("script timed out after {EXECUTION_TIMEOUT:?}"))?
        .context("event loop error during script execution")?;

        let duration_ms = started.elapsed().as_millis() as u64;
        emit_flux_event(serde_json::json!({
            "type": "execution_end",
            "id": execution_id,
            "status": "ok",
            "duration_ms": duration_ms,
        }));

        let state = self.runtime.op_state();
        let mut state = state.borrow_mut();
        let map = state.borrow_mut::<RuntimeStateMap>();
        let execution = map.remove(&execution_id);
        Ok(JsExecutionOutput {
            output: serde_json::Value::Null,
            checkpoints: execution
                .as_ref()
                .map(|e| e.checkpoints.clone())
                .unwrap_or_default(),
            error: None,
            logs: execution.as_ref().map(|e| e.logs.clone()).unwrap_or_default(),
            has_live_io: execution.as_ref().map(|e| e.has_live_io).unwrap_or(false),
            boundary_stop: None,
        })
    }
}

pub async fn boot_runtime_artifact(
    artifact: &RuntimeArtifact,
    context: ExecutionContext,
) -> Result<BootExecutionResult> {
    match artifact {
        RuntimeArtifact::Inline(artifact) => {
            boot_inline_runtime_artifact(&artifact.code, context).await
        }
        RuntimeArtifact::Built(artifact) => boot_built_runtime_artifact(artifact, context).await,
    }
}

async fn boot_inline_runtime_artifact(
    user_code: &str,
    context: ExecutionContext,
) -> Result<BootExecutionResult> {
    let started = std::time::Instant::now();
    let execution_id = context.execution_id.clone();
    let request_id = context.request_id.clone();
    let project_id = context.project_id.clone();
    let code_version = context.code_version.clone();
    let prepared = prepare_user_code(user_code);

    let main_specifier = ModuleSpecifier::parse("file:///flux_user_code.ts").unwrap();
    let transformed_entry = transpile_module_source(
        &main_specifier,
        MediaType::TypeScript,
        prepared,
        None,
    ).context("failed to transpile inline entry module")?;

    let mut modules = HashMap::new();
    let entry_source = transformed_entry.clone();
    let entry_sha256 = sha256_hex(entry_source.as_bytes());
    modules.insert(main_specifier.to_string(), ArtifactModule {
        specifier: main_specifier.to_string(),
        base_specifier: main_specifier.to_string(),
        source_kind: ArtifactSourceKind::Local,
        media_type: ArtifactMediaType::TypeScript,
        sha256: entry_sha256,
        source: entry_source.clone(),
        size_bytes: entry_source.len(),
        dependencies: Vec::new(),
    });

    let mut runtime = JsRuntime::new(RuntimeOptions {
        module_loader: Some(Rc::new(ArtifactModuleLoader { 
            source_maps: Rc::new(RefCell::new(HashMap::new())),
            modules,
        })),
        extensions: flux_extensions(),
        create_params: Some(deno_core::v8::CreateParams::default().heap_limits(0, V8_HEAP_LIMIT)),
        ..Default::default()
    });

    {
        let state = runtime.op_state();
        let mut state = state.borrow_mut();
        state.put::<RuntimeStateMap>(HashMap::new());
        state.borrow_mut::<RuntimeStateMap>().insert(
            execution_id.clone(),
            RuntimeExecutionState {
                context,
                call_index: 0,
                checkpoints: Vec::new(),
                recorded: HashMap::new(),
                recorded_now_ms: None,
                logs: Vec::new(),
                has_live_io: false,
                boundary_stop: None,
                recorded_random: Vec::new(),
                random_index: 0,
                recorded_uuids: Vec::new(),
                uuid_index: 0,
                is_server_mode: false,
                pending_responses: HashMap::new(),
                postgres_sessions: HashMap::new(),
                next_postgres_session_id: 0,
            },
        );
    }

    runtime
        .execute_script("flux:bootstrap_fetch", bootstrap_fetch_js())
        .context("failed to install fetch interceptor")?;

    runtime
        .execute_script("flux:bootstrap", bootstrap_js())
        .context("failed to install bootstrap globals")?;

    let eid_json =
        serde_json::to_string(&execution_id).context("failed to encode boot execution_id")?;
    runtime
        .execute_script(
            "flux:set_boot_eid",
            format!("globalThis.__FLUX_EXECUTION_ID__ = {eid_json};"),
        )
        .context("failed to set boot execution_id")?;

    let mut error = None;
    let module_id = match runtime
        .load_main_es_module_from_code(&main_specifier, transformed_entry)
        .await
    {
        Ok(id) => Some(id),
        Err(err) => {
            error = Some(format!("{err:#}"));
            None
        }
    };

    if let Some(id) = module_id {
        let evaluation = runtime.mod_evaluate(id);
        match tokio::time::timeout(
            EXECUTION_TIMEOUT,
            runtime.run_event_loop(Default::default()),
        )
        .await
        {
            Err(_) => {
                error = Some(format!(
                    "module initialization timed out after {EXECUTION_TIMEOUT:?}"
                ));
            }
            Ok(Err(err)) => {
                error = Some(format!("{err:#}"));
            }
            Ok(Ok(())) => {
                if let Err(err) = evaluation.await {
                    error = Some(format!("{err:#}"));
                }
            }
        }
    }

    close_registration_phase(&mut runtime)?;

    let is_server_mode = probe_server_mode(&mut runtime).unwrap_or(false);
    let has_handler = probe_handler(&mut runtime).unwrap_or(false);
    let (checkpoints, logs) = take_execution_artifacts(&mut runtime, &execution_id);

    Ok(BootExecutionResult {
        result: ExecutionResult {
            execution_id,
            request_id,
            project_id,
            code_version,
            status: if error.is_some() {
                "error".to_string()
            } else {
                "ok".to_string()
            },
            body: serde_json::json!({
                "phase": "boot",
                "listener_mode": is_server_mode,
                "has_handler": has_handler,
            }),
            error,
            duration_ms: started.elapsed().as_millis() as i32,
            checkpoints,
            logs,
            has_live_io: false,
        },
        is_server_mode,
        has_handler,
    })
}

async fn boot_built_runtime_artifact(
    artifact: &FluxBuildArtifact,
    context: ExecutionContext,
) -> Result<BootExecutionResult> {
    let started = std::time::Instant::now();
    let execution_id = context.execution_id.clone();
    let request_id = context.request_id.clone(); let project_id = context.project_id.clone();
    let code_version = context.code_version.clone();
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
        extensions: flux_extensions(),
        create_params: Some(deno_core::v8::CreateParams::default().heap_limits(0, V8_HEAP_LIMIT)),
        ..Default::default()
    });

    {
        let state = runtime.op_state();
        let mut state = state.borrow_mut();
        state.put::<RuntimeStateMap>(HashMap::new());
        state.borrow_mut::<RuntimeStateMap>().insert(
            execution_id.clone(),
            RuntimeExecutionState {
                context,
                call_index: 0,
                checkpoints: Vec::new(),
                recorded: HashMap::new(),
                recorded_now_ms: None,
                logs: Vec::new(),
                has_live_io: false,
                boundary_stop: None,
                recorded_random: Vec::new(),
                random_index: 0,
                recorded_uuids: Vec::new(),
                uuid_index: 0,
                is_server_mode: false,
                pending_responses: HashMap::new(),
                postgres_sessions: HashMap::new(),
                next_postgres_session_id: 0,
            },
        );
    }

    runtime
        .execute_script("flux:bootstrap_fetch", bootstrap_fetch_js())
        .context("failed to install fetch interceptor")?;

    runtime
        .execute_script("flux:bootstrap", bootstrap_js())
        .context("failed to install bootstrap globals")?;

    let eid_json =
        serde_json::to_string(&execution_id).context("failed to encode boot execution_id")?;
    runtime
        .execute_script(
            "flux:set_boot_eid",
            format!("globalThis.__FLUX_EXECUTION_ID__ = {eid_json};"),
        )
        .context("failed to set boot execution_id")?;

    let entry_module = artifact
        .modules
        .iter()
        .find(|module| module.specifier == artifact.entry_specifier)
        .ok_or_else(|| anyhow::anyhow!("entry module missing from built artifact"))?;
    let main_module = Url::parse(&artifact.entry_specifier).with_context(|| {
        format!(
            "invalid entry module specifier: {}",
            artifact.entry_specifier
        )
    })?;
    let transformed_entry = prepare_user_code(&entry_module.source);
    let transformed_entry = transpile_module_source(
        &main_module,
        artifact_media_type(entry_module.media_type.clone()),
        transformed_entry,
        None,
    )
    .context("failed to transpile built artifact entry module")?;

    let mut error = None;
    let module_id = match runtime
        .load_main_es_module_from_code(&main_module, transformed_entry)
        .await
    {
        Ok(module_id) => Some(module_id),
        Err(err) => {
            error = Some(format!("{err:#}"));
            None
        }
    };

    if let Some(module_id) = module_id {
        let evaluation = runtime.mod_evaluate(module_id);
        match tokio::time::timeout(
            EXECUTION_TIMEOUT,
            runtime.run_event_loop(Default::default()),
        )
        .await
        {
            Err(_) => {
                error = Some(format!(
                    "module initialization timed out after {EXECUTION_TIMEOUT:?}"
                ));
            }
            Ok(Err(err)) => {
                error = Some(format!("{err:#}"));
            }
            Ok(Ok(())) => {
                if let Err(err) = evaluation.await {
                    error = Some(format!("{err:#}"));
                }
            }
        }
    }

    close_registration_phase(&mut runtime)?;

    let is_server_mode = probe_server_mode(&mut runtime).unwrap_or(false);
    let has_handler = probe_handler(&mut runtime).unwrap_or(false);
    let (checkpoints, logs) = take_execution_artifacts(&mut runtime, &execution_id);

    Ok(BootExecutionResult {
        result: ExecutionResult {
            execution_id,
            request_id,
            project_id,
            code_version,
            status: if error.is_some() {
                "error".to_string()
            } else {
                "ok".to_string()
            },
            body: serde_json::json!({
                "phase": "boot",
                "listener_mode": is_server_mode,
                "has_handler": has_handler,
            }),
            error,
            duration_ms: started.elapsed().as_millis() as i32,
            checkpoints,
            logs,
            has_live_io: false,
        },
        is_server_mode,
        has_handler,
    })
}

fn probe_server_mode(runtime: &mut JsRuntime) -> Result<bool> {
    let probe = runtime
        .execute_script(
            "flux:probe_server_mode",
            "typeof globalThis.__flux_net_handler === 'function'",
        )
        .context("failed to probe server mode")?;
    deno_core::scope!(scope, runtime);
    let local = deno_core::v8::Local::new(scope, probe);
    Ok(local.is_true())
}

fn probe_handler(runtime: &mut JsRuntime) -> Result<bool> {
    let probe = runtime
        .execute_script(
            "flux:probe_handler",
            "typeof globalThis.__flux_user_handler === 'function'",
        )
        .context("failed to probe handler")?;
    deno_core::scope!(scope, runtime);
    let local = deno_core::v8::Local::new(scope, probe);
    Ok(local.is_true())
}

fn close_registration_phase(runtime: &mut JsRuntime) -> Result<()> {
    runtime
        .execute_script(
            "flux:close_registration_phase",
            "globalThis.__flux_registration_open = false;",
        )
        .context("failed to close listener registration phase")?;
    Ok(())
}

fn take_execution_artifacts(
    runtime: &mut JsRuntime,
    execution_id: &str,
) -> (Vec<FetchCheckpoint>, Vec<LogEntry>) {
    let state = runtime.op_state();
    let mut state = state.borrow_mut();
    let map = state.borrow_mut::<RuntimeStateMap>();
    match map.remove(execution_id) {
        Some(execution) => (execution.checkpoints, execution.logs),
        None => (Vec::new(), Vec::new()),
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
const __fluxBase64Alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

function __fluxNormalizeHeaderName(name) {
    return String(name).toLowerCase();
}

function __fluxUint8ArrayToB64(bytes) {
    let binary = "";
    for (let i = 0; i < bytes.byteLength; i++) {
        binary += String.fromCharCode(bytes[i]);
    }
    return globalThis.btoa(binary);
}

function __fluxB64ToUint8Array(b64) {
    const binaryString = globalThis.atob(b64);
    const bytes = new Uint8Array(binaryString.length);
    for (let i = 0; i < binaryString.length; i++) {
        bytes[i] = binaryString.charCodeAt(i);
    }
    return bytes;
}

function __fluxBodyToText(body) {
    if (body == null) return null;
    if (typeof body === "string") return body;
    if (body instanceof Uint8Array || body instanceof ArrayBuffer) {
        const bytes = body instanceof Uint8Array ? body : new Uint8Array(body);
        return "__FLUX_B64:" + __fluxUint8ArrayToB64(bytes);
    }
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
    if (state.bodyText?.startsWith("__FLUX_B64:")) {
        return __fluxDecoder.decode(__fluxB64ToUint8Array(state.bodyText.slice(11)));
    }
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
                    let value;
                    if (state.bodyText?.startsWith("__FLUX_B64:")) {
                        value = __fluxB64ToUint8Array(state.bodyText.slice(11));
                    } else {
                        value = __fluxEncoder.encode(state.bodyText || "");
                    }
                    return {
                        value,
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

function __fluxBase64Encode(input) {
    const source = String(input);
    const bytes = [];

    for (let index = 0; index < source.length; index++) {
        const codePoint = source.charCodeAt(index);
        if (codePoint > 0xFF) {
            throw new DOMException(
                "The string to be encoded contains characters outside of the Latin1 range.",
                "InvalidCharacterError",
            );
        }
        bytes.push(codePoint);
    }

    let output = "";
    for (let index = 0; index < bytes.length; index += 3) {
        const byte1 = bytes[index];
        const byte2 = index + 1 < bytes.length ? bytes[index + 1] : undefined;
        const byte3 = index + 2 < bytes.length ? bytes[index + 2] : undefined;
        const chunk = (byte1 << 16) | ((byte2 ?? 0) << 8) | (byte3 ?? 0);

        output += __fluxBase64Alphabet[(chunk >> 18) & 0x3F];
        output += __fluxBase64Alphabet[(chunk >> 12) & 0x3F];
        output += byte2 === undefined ? "=" : __fluxBase64Alphabet[(chunk >> 6) & 0x3F];
        output += byte3 === undefined ? "=" : __fluxBase64Alphabet[chunk & 0x3F];
    }

    return output;
}

function __fluxBase64Decode(input) {
    const source = String(input).replace(/[\t\n\f\r ]+/g, "");
    if (source.length % 4 === 1) {
        throw new DOMException("The string to be decoded is not correctly encoded.", "InvalidCharacterError");
    }
    if (/[^A-Za-z0-9+/=]/.test(source) || /=[^=]/.test(source) || /={3,}/.test(source)) {
        throw new DOMException("The string to be decoded is not correctly encoded.", "InvalidCharacterError");
    }

    let output = "";
    for (let index = 0; index < source.length; index += 4) {
        const chunk = source.slice(index, index + 4);
        if (chunk.length === 0) break;

        const char1 = chunk[0] ?? "A";
        const char2 = chunk[1] ?? "A";
        const char3 = chunk[2] ?? "A";
        const char4 = chunk[3] ?? "A";
        const value1 = __fluxBase64Alphabet.indexOf(char1);
        const value2 = __fluxBase64Alphabet.indexOf(char2);
        const value3 = char3 === "=" ? 0 : __fluxBase64Alphabet.indexOf(char3);
        const value4 = char4 === "=" ? 0 : __fluxBase64Alphabet.indexOf(char4);

        if (value1 === -1 || value2 === -1 || (char3 !== "=" && value3 === -1) || (char4 !== "=" && value4 === -1)) {
            throw new DOMException("The string to be decoded is not correctly encoded.", "InvalidCharacterError");
        }

        const combined = (value1 << 18) | (value2 << 12) | (value3 << 6) | value4;
        output += String.fromCharCode((combined >> 16) & 0xFF);
        if (char3 !== "=") {
            output += String.fromCharCode((combined >> 8) & 0xFF);
        }
        if (char4 !== "=") {
            output += String.fromCharCode(combined & 0xFF);
        }
    }

    return output;
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
        this._commit();
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
            this._commit();
        }
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

    async arrayBuffer() {
        if (this._bodyState.used) throw new TypeError("Body already consumed");
        this._bodyState.used = true;
        this._bodyState.emitted = true;
        const text = this._bodyState.bodyText;
        if (text?.startsWith("__FLUX_B64:")) {
            return __fluxB64ToUint8Array(text.slice(11)).buffer;
        }
        return __fluxEncoder.encode(text ?? "").buffer;
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

    async arrayBuffer() {
        if (this._bodyState.used) throw new TypeError("Body already consumed");
        this._bodyState.used = true;
        this._bodyState.emitted = true;
        const text = this._bodyState.bodyText;
        if (text?.startsWith("__FLUX_B64:")) {
            return __fluxB64ToUint8Array(text.slice(11)).buffer;
        }
        return __fluxEncoder.encode(text ?? "").buffer;
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

class Blob {
    constructor(parts = [], options = {}) {
        this._parts = Array.isArray(parts) ? parts : [parts];
        this.type = options.type || "";
        this.size = this._parts.reduce((acc, part) => {
            if (typeof part === "string") return acc + part.length;
            if (part instanceof ArrayBuffer) return acc + part.byteLength;
            if (ArrayBuffer.isView(part)) return acc + part.byteLength;
            if (part instanceof Blob) return acc + part.size;
            return acc + (part.length || 0);
        }, 0);
    }
    async arrayBuffer() {
        const total = new Uint8Array(this.size);
        let offset = 0;
        for (const part of this._parts) {
            let u8;
            if (typeof part === "string") {
                u8 = new TextEncoder().encode(part);
            } else if (part instanceof ArrayBuffer) {
                u8 = new Uint8Array(part);
            } else if (ArrayBuffer.isView(part)) {
                u8 = new Uint8Array(part.buffer, part.byteOffset, part.byteLength);
            } else if (part instanceof Blob) {
                u8 = new Uint8Array(await part.arrayBuffer());
            } else {
                u8 = new Uint8Array(part);
            }
            total.set(u8, offset);
            offset += u8.length;
        }
        return total.buffer;
    }
    async text() {
        return new TextDecoder().decode(await this.arrayBuffer());
    }
    slice(start, end, type) {
        return new Blob([], { type: type || this.type });
    }
}
globalThis.Blob = Blob;

globalThis.URLSearchParams = globalThis.URLSearchParams || URLSearchParams;
globalThis.URL = globalThis.URL || URL;
globalThis.DOMException = globalThis.DOMException || DOMException;
globalThis.AbortSignal = globalThis.AbortSignal || AbortSignal;
globalThis.AbortController = globalThis.AbortController || AbortController;
globalThis.Headers = Headers;
globalThis.FormData = globalThis.FormData || FormData;
globalThis.Request = Request;
globalThis.Response = Response;



// ── fetch ──────────────────────────────────────────────────────────────────
globalThis.fetch = async function(input, init = undefined) {
    if (!globalThis.__FLUX_EXECUTION_ID__) {
        throw new Error("Flux: IO outside execution context");
    }
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
    const bodyArray = (method === "GET" || method === "HEAD") ? null : await request.arrayBuffer();
    const body = bodyArray ? Flux.serializeArrayBuffer(bodyArray) : null;
    const response = await Deno.core.ops.op_flux_fetch({
        execution_id: __flux_eid(),
        url: String(request.url),
        method: String(method),
        body,
        headers,
    });

    const responseBody = response.body == null
        ? null
        : response.is_binary
            ? Flux.deserializeArrayBuffer(response.body)
            : typeof response.body === "string"
                ? response.body
                : JSON.stringify(response.body);

    return new Response(responseBody, {
        status: response.status,
        headers: new Headers(response.headers ?? {}),
    });
};
    "#
}

fn bootstrap_js() -> String {
    r#"
      function __flux_eid() {
        return globalThis.__FLUX_EXECUTION_ID__ || "__unknown__";
      }

// ── Date.now() + new Date() ────────────────────────────────────────────────
{
  const _OrigDate = globalThis.Date;
  class PatchedDate extends _OrigDate {
    constructor(...args) {
      if (args.length === 0) {
        if (!globalThis.__FLUX_EXECUTION_ID__) {
          throw new Error("Flux: Date IO outside execution context");
        }
        super(Deno.core.ops.op_flux_now(__flux_eid()));
      } else {
        super(...args);
      }
    }
  }
    PatchedDate.now = function() { 
      if (!globalThis.__FLUX_EXECUTION_ID__) {
        throw new Error("Flux: Date.now() outside execution context");
      }
      return Deno.core.ops.op_flux_now(__flux_eid()); 
    };
  globalThis.Date = PatchedDate;
}

// ── performance.now() ──────────────────────────────────────────────────────
if (globalThis.performance) {
    globalThis.performance.now = function() { 
      if (!globalThis.__FLUX_EXECUTION_ID__) {
        throw new Error("Flux: performance.now() outside execution context");
      }
      return Deno.core.ops.op_flux_now_high_res(__flux_eid()); 
    };
}

// ── console ────────────────────────────────────────────────────────────────
function _flux_fmt(...args) {
  return args.map(v => {
    if (typeof v === "string") return v;
    if (v instanceof Error) return v.stack || v.message || String(v);
    if (v === null) return "null";
    if (v === undefined) return "undefined";
    try {
      const s = JSON.stringify(v);
      // JSON.stringify returns "{}" for Error-like objects — fall back to String()
      if (s === "{}" && v && typeof v === "object" && !Array.isArray(v) && Object.keys(v).length === 0) {
        const str = String(v);
        return str === "[object Object]" ? s : str;
      }
      return s;
    } catch (_) {
      return String(v);
    }
  }).join(" ");
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

// ── WinterTC minimum-common globals ────────────────────────────────────────
if (typeof globalThis.btoa !== "function") {
    globalThis.btoa = (input) => __fluxBase64Encode(input);
}

if (typeof globalThis.atob !== "function") {
    globalThis.atob = (input) => __fluxBase64Decode(input);
}

if (!("self" in globalThis) || globalThis.self !== globalThis) {
    Object.defineProperty(globalThis, "self", {
        value: globalThis,
        writable: true,
        enumerable: true,
        configurable: true,
    });
}

if (!("global" in globalThis) || globalThis.global !== globalThis) {
    Object.defineProperty(globalThis, "global", {
        value: globalThis,
        writable: true,
        enumerable: false,
        configurable: true,
    });
}

const __fluxProcessEnv = new Proxy({}, {
    get(_target, prop) {
        if (typeof prop !== "string") return undefined;
        return Deno.core.ops.op_flux_env_get(prop);
    },
    has(_target, prop) {
        if (typeof prop !== "string") return false;
        return Deno.core.ops.op_flux_env_get(prop) != null;
    },
    ownKeys() {
        return [];
    },
    getOwnPropertyDescriptor(_target, prop) {
        if (typeof prop !== "string") return undefined;
        const value = Deno.core.ops.op_flux_env_get(prop);
        if (value == null) return undefined;
        return {
            configurable: true,
            enumerable: true,
            writable: false,
            value,
        };
    },
});

if (typeof globalThis.process !== "object" || globalThis.process === null) {
    Object.defineProperty(globalThis, "process", {
        value: {
            env: __fluxProcessEnv,
            argv: [],
            cwd: () => "/",
            platform: "linux",
            versions: { node: "20.0.0", flux: "1" },
            nextTick: (callback, ...args) => queueMicrotask(() => callback(...args)),
        },
        writable: true,
        enumerable: false,
        configurable: true,
    });
} else if (typeof globalThis.process.env !== "object" || globalThis.process.env === null) {
    globalThis.process.env = __fluxProcessEnv;
}

const __fluxNavigator = typeof globalThis.navigator === "object" && globalThis.navigator !== null
    ? globalThis.navigator
    : {};

if (typeof __fluxNavigator.userAgent !== "string" || __fluxNavigator.userAgent.length === 0) {
    Object.defineProperty(__fluxNavigator, "userAgent", {
        value: "Flux Runtime",
        writable: false,
        enumerable: true,
        configurable: true,
    });
}

if (globalThis.navigator !== __fluxNavigator) {
    Object.defineProperty(globalThis, "navigator", {
        value: __fluxNavigator,
        writable: true,
        enumerable: true,
        configurable: true,
    });
}

if (typeof globalThis.reportError !== "function") {
    globalThis.reportError = (error) => {
        if (error instanceof Error) {
            console.error(error.stack || `${error.name}: ${error.message}`);
            return;
        }
        console.error(error);
    };
}

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
    if (!globalThis.__FLUX_EXECUTION_ID__) {
        throw new Error("Flux: IO outside execution context");
    }
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
    if (!globalThis.__FLUX_EXECUTION_ID__) {
        throw new Error("Flux: IO outside execution context");
    }
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
Math.random = () => {
    if (!globalThis.__FLUX_EXECUTION_ID__) {
        throw new Error("Flux: IO outside execution context");
    }
    return Deno.core.ops.op_random(__flux_eid());
};

// ── Flux.net (deterministic outbound TCP) ───────────────────────────────────
globalThis.Flux = globalThis.Flux || {};
globalThis.Flux.postgres = globalThis.Flux.postgres || {};
globalThis.Flux.redis = globalThis.Flux.redis || {};
globalThis.Flux.net = globalThis.Flux.net || {};
const __flux_pg_builtin_types = Object.freeze({
    BOOL: 16,
    BYTEA: 17,
    INT8: 20,
    INT2: 21,
    INT4: 23,
    TEXT: 25,
    OID: 26,
    JSON: 114,
    FLOAT4: 700,
    FLOAT8: 701,
    VARCHAR: 1043,
    BOOL_ARRAY: 1000,
    INT2_ARRAY: 1005,
    INT4_ARRAY: 1007,
    TEXT_ARRAY: 1009,
    INT8_ARRAY: 1016,
    FLOAT4_ARRAY: 1021,
    FLOAT8_ARRAY: 1022,
    DATE: 1082,
    TIME: 1083,
    TIMESTAMP: 1114,
    TIMESTAMPTZ: 1184,
    INTERVAL: 1186,
    TIMETZ: 1266,
    VARCHAR_ARRAY: 1015,
    NUMERIC: 1700,
    NUMERIC_ARRAY: 1231,
    UUID: 2950,
    JSONB: 3802,
});
const __flux_pg_type_parsers = new Map();
function __flux_pg_parser_key(typeId, format) {
    return `${String(typeId)}:${format === "binary" ? "binary" : "text"}`;
}
function __flux_pg_set_type_parser(typeId, formatOrParser, parserMaybe) {
    const parser = typeof parserMaybe === "function" ? parserMaybe : formatOrParser;
    const format = typeof parserMaybe === "function"
        ? (formatOrParser === "binary" ? "binary" : "text")
        : "text";
    if (typeof parser !== "function") {
        throw new TypeError("Flux.postgres.nodePgTypes.setTypeParser expects a parser function");
    }
    __flux_pg_type_parsers.set(__flux_pg_parser_key(typeId, format), parser);
}
globalThis.Flux.postgres.nodePgTypes = Object.freeze({
    builtins: __flux_pg_builtin_types,
    getTypeParser(typeId, format = "text") {
        return __flux_pg_type_parsers.get(__flux_pg_parser_key(typeId, format)) ?? ((value) => value);
    },
    setTypeParser: __flux_pg_set_type_parser,
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
function __flux_node_pg_field_list(fields, rows) {
    if (Array.isArray(fields) && fields.length > 0) {
        return fields.map((field) => ({
            name: String(field?.name ?? ""),
            dataTypeID: Number(field?.dataTypeID ?? 0),
            format: field?.format === "binary" ? "binary" : "text",
        }));
    }

    const first = Array.isArray(rows) ? rows[0] : null;
    if (!first || typeof first !== "object" || Array.isArray(first)) return [];
    return Object.keys(first).map((name) => ({
        name,
        dataTypeID: 0,
        format: "text",
    }));
}
function __flux_node_pg_apply_field_parsers(row, fields) {
    if (!row || typeof row !== "object" || Array.isArray(row)) return row;
    const parsed = {};
    for (const field of fields) {
        const name = String(field?.name ?? "");
        const parser = globalThis.Flux.postgres.nodePgTypes.getTypeParser(field?.dataTypeID ?? 0, field?.format ?? "text");
        const value = row[name];
        parsed[name] = value == null ? value : parser(value);
    }
    return parsed;
}
function __flux_node_pg_rows(rows, fields, rowMode) {
    if (!Array.isArray(rows)) return [];
    const parsedRows = rows.map((row) => __flux_node_pg_apply_field_parsers(row, fields));
    if (rowMode !== "array") return parsedRows;
    return parsedRows.map((row) => {
        if (!row || typeof row !== "object" || Array.isArray(row)) return [];
        return fields.map((field) => row[String(field?.name ?? "")]);
    });
}
function __flux_node_pg_error(errorLike) {
    if (!errorLike || typeof errorLike !== "object") {
        return new Error("postgres query failed");
    }

    const error = new Error(String(errorLike.message ?? "postgres query failed"));
    error.name = "DatabaseError";
    for (const key of ["code", "detail", "constraint", "schema", "table", "column"]) {
        if (errorLike[key] != null) {
            error[key] = String(errorLike[key]);
        }
    }
    return error;
}
function __flux_node_pg_response_or_throw(response) {
    if (response?.error && typeof response.error === "object") {
        throw __flux_node_pg_error(response.error);
    }
    return response ?? {};
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
        __flux_node_pg_response_or_throw(response);
        const fields = __flux_node_pg_field_list(response.fields, response.rows);
        const rows = __flux_node_pg_rows(response.rows, fields, normalized.rowMode);
        return {
            command: response.command ?? null,
            rowCount: Number(response.rowCount ?? response.row_count ?? rows.length),
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
        __flux_node_pg_response_or_throw(response);
        const fields = __flux_node_pg_field_list(response.fields, response.rows);
        const rows = __flux_node_pg_rows(response.rows, fields, normalized.rowMode);
        return {
            command: response.command ?? null,
            rowCount: Number(response.rowCount ?? response.row_count ?? rows.length),
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
    __flux_node_pg_response_or_throw(response);

    return {
        rows: Array.isArray(response.rows) ? response.rows : [],
        fields: Array.isArray(response.fields) ? response.fields : [],
        command: response.command ?? null,
        rowCount: Number(response.rowCount ?? response.row_count ?? 0),
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
    __flux_node_pg_response_or_throw(response);

    return {
        rows: Array.isArray(response.rows) ? response.rows : [],
        fields: Array.isArray(response.fields) ? response.fields : [],
        command: response.command ?? null,
        rowCount: Number(response.rowCount ?? response.row_count ?? 0),
        replay: !!response.replay,
    };
};
function __flux_redis_response_or_throw(response) {
    if (response?.error && typeof response.error === "object") {
        const error = new Error(String(response.error.message ?? "redis command failed"));
        error.name = "RedisError";
        throw error;
    }
    return response ?? {};
}
function __flux_redis_unsupported(feature) {
    const verb = feature === "pub/sub" ? "is" : "are";
    throw new Error(`Redis ${feature} ${verb} not supported in Flux (non-deterministic execution)`);
}
class FluxRedisClient {
    constructor(options = {}) {
        this.url = String(options?.url ?? options?.connectionString ?? "");
        this.__closed = false;
    }

    async connect() {
        if (this.__closed) {
            throw new Error("Flux.redis client has already been closed");
        }
        return;
    }

    async sendCommand(parts) {
        if (this.__closed) {
            throw new Error("Flux.redis client has already been closed");
        }
        if (!Array.isArray(parts) || parts.length === 0) {
            throw new TypeError("Flux.redis sendCommand expects a non-empty command array");
        }
        const commandParts = parts.map((part) => String(part));
        if (!commandParts[0]) {
            throw new TypeError("Flux.redis sendCommand requires a command name");
        }

        const response = globalThis.Flux.redis.command({
            connectionString: this.url,
            command: commandParts[0],
            args: commandParts.slice(1),
        });
        return response.value;
    }

    async get(key) {
        return this.sendCommand(["GET", key]);
    }

    async set(key, value) {
        return this.sendCommand(["SET", key, value]);
    }

    async del(...keys) {
        return this.sendCommand(["DEL", ...keys]);
    }

    async exists(key) {
        return this.sendCommand(["EXISTS", key]);
    }

    async incr(key) {
        return this.sendCommand(["INCR", key]);
    }

    async decr(key) {
        return this.sendCommand(["DECR", key]);
    }

    async hGet(key, field) {
        return this.sendCommand(["HGET", key, field]);
    }

    async hSet(key, field, value) {
        return this.sendCommand(["HSET", key, field, value]);
    }

    async hDel(key, field) {
        return this.sendCommand(["HDEL", key, field]);
    }

    async expire(key, seconds) {
        return this.sendCommand(["EXPIRE", key, seconds]);
    }

    async ttl(key) {
        return this.sendCommand(["TTL", key]);
    }

    multi() {
        return __flux_redis_unsupported("transactions");
    }

    exec() {
        return __flux_redis_unsupported("transactions");
    }

    watch() {
        return __flux_redis_unsupported("transactions");
    }

    unwatch() {
        return __flux_redis_unsupported("transactions");
    }

    subscribe() {
        return __flux_redis_unsupported("pub/sub");
    }

    psubscribe() {
        return __flux_redis_unsupported("pub/sub");
    }

    publish() {
        return __flux_redis_unsupported("pub/sub");
    }

    unsubscribe() {
        return __flux_redis_unsupported("pub/sub");
    }

    pipeline() {
        return __flux_redis_unsupported("pipelines");
    }

    batch() {
        return __flux_redis_unsupported("pipelines");
    }

    async quit() {
        this.__closed = true;
        return;
    }

    async disconnect() {
        this.__closed = true;
        return;
    }
}
globalThis.Flux.redis.command = function(options = {}) {
    const response = Deno.core.ops.op_flux_redis_command({
        execution_id: __flux_eid(),
        connection_string: String(options.connectionString ?? options.url ?? ""),
        command: String(options.command ?? ""),
        args: Array.isArray(options.args) ? options.args : [],
    });
    __flux_redis_response_or_throw(response);
    return {
        value: response.value ?? null,
        replay: !!response.replay,
    };
};
globalThis.Flux.redis.FluxRedisClient = FluxRedisClient;
globalThis.Flux.redis.createClient = function(options = {}) {
    return new FluxRedisClient(options);
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
if (!Deno.env) {
    Deno.env = {};
}

if (typeof Deno.env.get !== "function") {
    Deno.env.get = function(key) {
        return Deno.core.ops.op_flux_env_get(String(key));
    };
}

if (typeof Deno.env.toObject !== "function") {
    Deno.env.toObject = function() {
        return Deno.core.ops.op_flux_env_list();
    };
}

globalThis.__flux_net_handler = null;
globalThis.__flux_user_handler = null; // New: holds exported default function
globalThis.__flux_net_server = null;
globalThis.__flux_registration_open = true;
globalThis.__flux_listener_registered = false;

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
    if (!globalThis.__flux_registration_open) {
        throw new Error("Deno.serve may only be called during boot");
    }
    if (globalThis.__flux_listener_registered) {
        throw new Error("Deno.serve may only register one listener during boot");
    }

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

    globalThis.__flux_listener_registered = true;
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
  globalThis.__flux_last_error = globalThis.__flux_last_error || {};
    const serverState = globalThis.__flux_net_server;
    
    // The handler can be from Deno.serve() OR a direct export default function.
    // If it's an exported function, we wrap it in the FluxContext.
    const isServerMode = !!(serverState && !serverState.closed);
    const handler = isServerMode ? serverState.handler : globalThis.__flux_user_handler;

    if (serverState && serverState.closed) {
        const message = serverState.reason == null ? "Server closed" : String(serverState.reason);
        Deno.core.ops.op_net_respond(__eid, reqId, 503, "[]", message);
        return;
    }
  if (!handler) {
    Deno.core.ops.op_net_respond(__eid, reqId, 500, "[]", "No request handler registered");
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
    if (isServerMode) {
        // Deno.serve mode: handler(request)
        response = await handler(request);
    } else {
        // Flux Function mode: handler(ctx)
        const ctx = {
            req: request,
            env: Deno.env.toObject(),
            project_id: globalThis.__FLUX_PROJECT_ID__,
            json: (data, init) => {
              const body = JSON.stringify(data);
              const headers = new Headers(init?.headers);
              if (!headers.has("content-type")) {
                headers.set("content-type", "application/json");
              }
              return new Response(body, { ...init, headers });
            },
            text: (data, init) => new Response(String(data), init),
            html: (data, init) => {
              const headers = new Headers(init?.headers);
              if (!headers.has("content-type")) {
                headers.set("content-type", "text/html; charset=utf-8");
              }
              return new Response(String(data), { ...init, headers });
            }
        };
        response = await handler(ctx);
    }
  } catch (err) {
    const msg = String(err && err.stack ? err.stack : err);
    globalThis.__flux_last_error[__eid] = msg;
    Deno.core.ops.op_net_respond(__eid, reqId, 500, "[]", msg);
    return;
  }

    if (!(response instanceof Response)) {
        response = new Response(response == null ? "" : response);
    }

  let responseBody;
  try {
    const buffer = await response.arrayBuffer();
    const bytes = new Uint8Array(buffer);
    responseBody = "__FLUX_B64:" + __fluxUint8ArrayToB64(bytes);
  } catch (err) {
    responseBody = "";
  }

  const responseHeaders = JSON.stringify([...response.headers.entries()]);
  Deno.core.ops.op_net_respond(__eid, reqId, response.status ?? 200, responseHeaders, responseBody);
};
    "# .to_string()
}

fn prepare_user_code(code: &str) -> String {
    rewrite_export_default(code)
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
