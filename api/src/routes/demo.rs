use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use crate::AppState;

// ── Helpers ───────────────────────────────────────────────────────────────────

fn db_err() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": "database_error" })),
    )
}

fn bad_req(msg: &'static str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
}

fn rate_limited() -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::TOO_MANY_REQUESTS,
        Json(serde_json::json!({
            "error": "rate_limited",
            "message": "Too many demo requests. Please wait a minute."
        })),
    )
}

// ── POST /demo/signup ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SignupPayload {
    pub name:  String,
    pub email: String,
}

#[derive(Serialize)]
pub struct SignupResponse {
    pub status:     &'static str,
    pub request_id: String,
    pub message:    String,
}

pub async fn demo_signup(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(payload): Json<SignupPayload>,
) -> Result<Json<SignupResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pool = &state.pool;

    // ── Extract client IP ─────────────────────────────────────────────────
    let ip = headers
        .get("x-forwarded-for")
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|h| h.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string());

    // ── Validate inputs ───────────────────────────────────────────────────
    let name = payload.name.trim().to_string();
    let email = payload.email.trim().to_lowercase();

    if name.is_empty() || name.len() > 120 {
        return Err(bad_req("invalid_name"));
    }
    if !email.contains('@') || email.len() > 254 {
        return Err(bad_req("invalid_email"));
    }

    // ── IP rate limit: 5 per 60 seconds ──────────────────────────────────
    let recent: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM demo_requests \
         WHERE ip = $1 AND created_at > NOW() - INTERVAL '60 seconds'",
    )
    .bind(&ip)
    .fetch_one(pool)
    .await
    .map_err(|_| db_err())?;

    if recent >= 5 {
        return Err(rate_limited());
    }

    // ── Generate request_id ───────────────────────────────────────────────
    let request_id = Uuid::new_v4().to_string().replace('-', "")[..12].to_string();

    // ── Persist demo_requests row ─────────────────────────────────────────
    sqlx::query(
        "INSERT INTO demo_requests (request_id, ip, email, name) VALUES ($1, $2, $3, $4)",
    )
    .bind(&request_id)
    .bind(&ip)
    .bind(&email)
    .bind(&name)
    .execute(pool)
    .await
    .map_err(|_| db_err())?;

    // ── Persist demo_users row (real DB write) ────────────────────────────
    // We insert at the API layer because ctx.db is not yet wired in the
    // runtime executor.  The function still logs a timing span for the trace.
    let _ = sqlx::query(
        "INSERT INTO demo_users (name, email) VALUES ($1, $2) \
         ON CONFLICT DO NOTHING",
    )
    .bind(&name)
    .bind(&email)
    .execute(pool)
    .await;

    // ── Manually insert a gateway-style span so the trace starts correctly ─
    // The real gateway span is inserted when the POST /create_user call hits
    // the gateway proxy — fire-and-forget below takes care of that chain.
    // We pre-insert a lightweight "request received" span here so the trace
    // endpoint can immediately return status="running" even before the gateway
    // responds.
    let demo_tid: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM tenants WHERE slug = $1 LIMIT 1",
    )
    .bind(
        std::env::var("DEMO_TENANT_SLUG")
            .unwrap_or_else(|_| "demo".to_string()),
    )
    .fetch_optional(pool)
    .await
    .unwrap_or(None);

    if let Some(tid) = demo_tid {
        let rid = request_id.clone();
        let pool2 = pool.clone();
        tokio::spawn(async move {
            let _ = sqlx::query(
                "INSERT INTO platform_logs \
                 (id, tenant_id, source, resource_id, level, message, request_id, span_type) \
                 VALUES ($1, $2, 'gateway', 'create_user', 'info', \
                         'route matched: POST /create_user', $3, 'start')",
            )
            .bind(Uuid::new_v4())
            .bind(tid)
            .bind(&rid)
            .execute(&pool2)
            .await;
        });
    }

    // ── Fire-and-forget: invoke the real function via gateway ─────────────
    let gateway_url = std::env::var("GATEWAY_URL")
        .unwrap_or_else(|_| state.gateway_url.clone());
    let demo_slug = std::env::var("DEMO_TENANT_SLUG")
        .unwrap_or_default();

    if !demo_slug.is_empty() {
        let client = state.http_client.clone();
        let rid = request_id.clone();
        let email_clone = email.clone();
        let name_clone = name.clone();

        tokio::spawn(async move {
            let invoke_url = format!("{}/create_user", gateway_url.trim_end_matches('/'));
            let _ = client
                .post(&invoke_url)
                .header("x-tenant", &demo_slug)
                .header("x-request-id", &rid)
                .header("content-type", "application/json")
                .json(&serde_json::json!({
                    "name":       name_clone,
                    "email":      email_clone,
                    "REQUEST_ID": rid,
                }))
                .timeout(std::time::Duration::from_secs(30))
                .send()
                .await;
        });
    }

    Ok(Json(SignupResponse {
        status:     "ok",
        request_id: request_id.clone(),
        message:    format!("Demo running. Trace ID: {}", request_id),
    }))
}

// ── GET /demo/trace/:request_id ───────────────────────────────────────────────

#[derive(Serialize)]
pub struct TraceSpan {
    pub name:        String,
    pub duration_ms: i64,
    pub source:      String,
}

#[derive(Serialize)]
pub struct DemoTraceResponse {
    /// "pending" | "running" | "ready"
    pub status:           &'static str,
    pub request_id:       String,
    pub spans:            Vec<TraceSpan>,
    pub total_duration_ms: Option<i64>,
    pub email:            Option<String>,
}

pub async fn demo_trace(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
) -> Result<Json<DemoTraceResponse>, (StatusCode, Json<serde_json::Value>)> {
    let pool = &state.pool;

    // ── Verify this request_id was created by POST /demo/signup ──────────
    // Simple enumeration guard — prevents anyone from probing arbitrary
    // request IDs out of `platform_logs`.
    let demo_row: Option<(String,)> = sqlx::query_as(
        "SELECT email FROM demo_requests WHERE request_id = $1 LIMIT 1",
    )
    .bind(&request_id)
    .fetch_optional(pool)
    .await
    .map_err(|_| db_err())?;

    let email = match demo_row {
        Some((e,)) => e,
        None => {
            return Err((
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "not_found" })),
            ))
        }
    };

    // ── Fetch all spans for this request_id ───────────────────────────────
    #[derive(sqlx::FromRow)]
    struct SpanRow {
        source:    String,
        message:   String,
        span_type: Option<String>,
        timestamp: chrono::DateTime<chrono::Utc>,
    }

    let rows = sqlx::query_as::<_, SpanRow>(
        "SELECT source, message, span_type, timestamp \
         FROM platform_logs \
         WHERE request_id = $1 \
         ORDER BY timestamp ASC",
    )
    .bind(&request_id)
    .fetch_all(pool)
    .await
    .map_err(|_| db_err())?;

    if rows.is_empty() {
        return Ok(Json(DemoTraceResponse {
            status:            "pending",
            request_id,
            spans:             vec![],
            total_duration_ms: None,
            email:             Some(email),
        }));
    }

    // ── Shape spans for the terminal UI ───────────────────────────────────
    // Rules:
    //   source=gateway           → "gateway.route"
    //   source=function, start   → "create_user"
    //   span_type=workflow_step  → parse "workflow:NAME  Nms" → name + duration
    //   source=tool              → parse "tool:NAME  Nms"     → name + duration
    //   everything else          → skip

    let first_ts = rows[0].timestamp;
    let last_ts  = rows[rows.len() - 1].timestamp;
    let total_ms = (last_ts - first_ts).num_milliseconds().max(1);

    let mut spans: Vec<TraceSpan> = Vec::new();
    let mut has_tool = false;
    let mut has_function_end = false;

    for (i, row) in rows.iter().enumerate() {
        let span_type = row.span_type.as_deref().unwrap_or("event");

        match row.source.as_str() {
            // ── Gateway routing span ──────────────────────────────────────
            "gateway" => {
                let next_ts = rows.get(i + 1).map(|r| r.timestamp).unwrap_or(last_ts);
                let dur = (next_ts - row.timestamp).num_milliseconds().max(1);
                spans.push(TraceSpan {
                    name:        "gateway.route".to_string(),
                    duration_ms: dur,
                    source:      "gateway".to_string(),
                });
            }

            // ── Function start / end ──────────────────────────────────────
            "function" if span_type == "start" => {
                // Duration = total function time = (last_ts - this span)
                let dur = (last_ts - row.timestamp).num_milliseconds().max(1);
                spans.push(TraceSpan {
                    name:        "create_user".to_string(),
                    duration_ms: dur,
                    source:      "function".to_string(),
                });
            }
            "function" if span_type == "end" => {
                has_function_end = true;
            }

            // ── Workflow steps ────────────────────────────────────────────
            "workflow" if span_type == "workflow_step" => {
                // Message format: "workflow:STEP_NAME  Nms"
                let (step_name, dur) = parse_workflow_span(&row.message);
                // Only show non-wrapper workflow steps (not the email-only step)
                spans.push(TraceSpan {
                    name:        step_name,
                    duration_ms: dur,
                    source:      "workflow".to_string(),
                });
            }

            // ── Tool call spans ───────────────────────────────────────────
            "tool" if span_type == "tool" => {
                let (tool_name, dur) = parse_tool_span(&row.message);
                spans.push(TraceSpan {
                    name:        tool_name,
                    duration_ms: dur,
                    source:      "tool".to_string(),
                });
                has_tool = true;
            }

            _ => {} // skip internal/duplicate entries
        }
    }

    // Deduplicate: remove workflow.send_welcome if outlook.send_email appeared
    // (the workflow step wraps the tool call — showing both is redundant for the
    // demo UI but we keep the tool span since it has the true network duration).
    if has_tool {
        spans.retain(|s| s.name != "workflow.send_welcome");
    }

    // ── Status ────────────────────────────────────────────────────────────
    let status: &'static str = if has_function_end || has_tool {
        "ready"
    } else if !spans.is_empty() {
        "running"
    } else {
        "pending"
    };

    Ok(Json(DemoTraceResponse {
        status,
        request_id,
        spans,
        total_duration_ms: Some(total_ms),
        email: Some(email),
    }))
}

// ── Span message parsers ──────────────────────────────────────────────────────

/// Parse "workflow:db.insert(users)  12ms" → ("db.insert(users)", 12)
fn parse_workflow_span(msg: &str) -> (String, i64) {
    // Strip "workflow:" prefix
    let body = msg.strip_prefix("workflow:").unwrap_or(msg);
    // Split on "  " to separate name from "Nms"
    if let Some(idx) = body.rfind("  ") {
        let name = body[..idx].trim().to_string();
        let dur_str = body[idx..].trim().trim_end_matches("ms");
        let dur = dur_str.parse::<i64>().unwrap_or(0);
        return (name, dur);
    }
    (body.trim().to_string(), 0)
}

/// Parse "tool:outlook.send_email  89ms" → ("outlook.send_email", 89)
fn parse_tool_span(msg: &str) -> (String, i64) {
    let body = msg.strip_prefix("tool:").unwrap_or(msg);
    if let Some(idx) = body.rfind("  ") {
        let name = body[..idx].trim().to_string();
        let dur_str = body[idx..].trim().trim_end_matches("ms");
        let dur = dur_str.parse::<i64>().unwrap_or(0);
        return (name, dur);
    }
    (body.trim().to_string(), 0)
}
