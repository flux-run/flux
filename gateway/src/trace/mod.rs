//! Distributed tracing helpers.
//!
//! Responsibilities:
//!   1. `resolve_request_id`  — pick or synthesise the x-request-id
//!   2. `write_root`          — fire-and-forget DB write of the trace root span
use axum::http::HeaderMap;
use sqlx::PgPool;
use std::collections::HashMap;
use uuid::Uuid;
use crate::snapshot::RouteRecord;

// ── Request ID ──────────────────────────────────────────────────────────────

/// Return the request-scoped trace ID.
///
/// Precedence (first wins):
///   1. `x-request-id` header sent by the caller
///   2. `traceparent` trace_id parsed from the W3C header
///   3. Freshly generated UUID
pub fn resolve_request_id(headers: &HeaderMap) -> String {
    if let Some(id) = headers.get("x-request-id").and_then(|v| v.to_str().ok()) {
        return id.to_string();
    }
    if let Some(tp) = headers.get("traceparent")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_traceparent)
    {
        return tp.trace_id;
    }
    Uuid::new_v4().to_string()
}

/// Extract the parent-span ID from headers (optional).
///
/// Precedence: `x-parent-span-id` then W3C `traceparent` parent_id.
pub fn resolve_parent_span(headers: &HeaderMap) -> Option<String> {
    if let Some(id) = headers.get("x-parent-span-id").and_then(|v| v.to_str().ok()) {
        return Some(id.to_string());
    }
    headers.get("traceparent")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_traceparent)
        .map(|tp| tp.parent_id)
}

// ── Trace root write ─────────────────────────────────────────────────────────

/// Persist the trace root envelope to `trace_requests`.
///
/// Fire-and-forget via `tokio::spawn` — never blocks the hot path.
pub fn write_root(
    pool:       PgPool,
    route:      &RouteRecord,
    request_id: &str,
    method:     &str,
    path:       &str,
    headers:    HashMap<String, String>,
    body:       serde_json::Value,
) {
    let rid = request_id.to_string();
    let pid = route.project_id;
    let fid = route.function_id;
    let m   = method.to_string();
    let p   = path.to_string();

    tokio::spawn(async move {
        let _ = sqlx::query(
            "INSERT INTO trace_requests
             (request_id, project_id, function_id, method, path,
              headers, body, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
             ON CONFLICT (request_id) DO NOTHING",
        )
        .bind(&rid)
        .bind(pid)
        .bind(fid)
        .bind(&m)
        .bind(&p)
        .bind(serde_json::to_value(&headers).ok())
        .bind(body)
        .execute(&pool)
        .await;
    });
}

// ── W3C traceparent parser ────────────────────────────────────────────────────

struct Traceparent { trace_id: String, parent_id: String }

/// Parse a W3C `traceparent` header value per the Level 1 spec.
///
/// Returns `None` for any malformed, reserved, or all-zero value.
fn parse_traceparent(header: &str) -> Option<Traceparent> {
    let parts: Vec<&str> = header.trim().splitn(4, '-').collect();
    if parts.len() < 4 { return None; }

    let (version, trace_hex, parent_hex) = (parts[0], parts[1], parts[2]);

    if version == "ff"                           { return None; }
    if trace_hex.len()  != 32                   { return None; }
    if parent_hex.len() != 16                   { return None; }
    if !trace_hex.chars().all(|c| c.is_ascii_hexdigit())  { return None; }
    if !parent_hex.chars().all(|c| c.is_ascii_hexdigit()) { return None; }
    if trace_hex  == "00000000000000000000000000000000" { return None; }
    if parent_hex == "0000000000000000"          { return None; }

    Some(Traceparent {
        trace_id: format!(
            "{}-{}-{}-{}-{}",
            &trace_hex[0..8],  &trace_hex[8..12],
            &trace_hex[12..16], &trace_hex[16..20],
            &trace_hex[20..32],
        ),
        parent_id: parent_hex.to_lowercase(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_valid_traceparent_into_uuid() {
        let headers = {
            let mut m = HeaderMap::new();
            m.insert(
                "traceparent",
                "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
                    .parse().unwrap(),
            );
            m
        };
        assert_eq!(
            resolve_request_id(&headers),
            "4bf92f35-77b3-4da6-a3ce-929d0e0e4736",
        );
    }
    #[test]
    fn x_request_id_wins_over_traceparent() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id",  "my-id".parse().unwrap());
        headers.insert("traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".parse().unwrap());
        assert_eq!(resolve_request_id(&headers), "my-id");
    }
}
