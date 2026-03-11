use reqwest::Client;
use serde_json::Value;
use sqlx::PgPool;
use uuid::Uuid;

/// Dispatch a single event to a single subscription target.
/// Returns Ok(http_status) for webhooks, Ok(200) for function/queue targets.
/// Errors here are non-fatal — the worker records them in event_deliveries.
///
/// `request_id` is the originating request's trace ID, forwarded as
/// `x-request-id` on webhook and function calls to keep traces continuous.
pub async fn dispatch(
    pool: &PgPool,
    http: &Client,
    runtime_url: &str,
    _subscription_id: Uuid,
    target_type: &str,
    target_config: &Value,
    event_payload: &Value,
    event_type: &str,
    request_id: &str,
) -> Result<u16, String> {
    match target_type {
        "webhook"   => dispatch_webhook(http, target_config, event_payload, event_type, request_id).await,
        "function"  => dispatch_function(http, runtime_url, target_config, event_payload, request_id).await,
        "queue_job" => dispatch_queue_job(pool, target_config, event_payload).await,
        other => Err(format!("unknown target_type: {}", other)),
    }
}

// ─── Webhook dispatch ─────────────────────────────────────────────────────────

async fn dispatch_webhook(
    http: &Client,
    config: &Value,
    payload: &Value,
    event_type: &str,
    request_id: &str,
) -> Result<u16, String> {
    let url = config
        .get("url")
        .and_then(|v| v.as_str())
        .ok_or("webhook config missing 'url'")?;

    let mut req = http
        .post(url)
        .header("content-type", "application/json")
        .header("x-fluxbase-event", event_type)
        .header("x-request-id", request_id);

    // Optional HMAC signature header for endpoint verification.
    if let Some(secret) = config.get("secret").and_then(|v| v.as_str()) {
        let sig = hmac_sha256(secret, &payload.to_string());
        req = req.header("x-fluxbase-signature", format!("sha256={}", sig));
    }

    // Optional extra headers from config.
    if let Some(headers) = config.get("headers").and_then(|v| v.as_object()) {
        for (k, v) in headers {
            if let Some(val) = v.as_str() {
                req = req.header(k.as_str(), val);
            }
        }
    }

    let resp = req
        .json(payload)
        .send()
        .await
        .map_err(|e| format!("webhook request failed: {}", e))?;

    let status = resp.status().as_u16();
    if status >= 200 && status < 300 {
        Ok(status)
    } else {
        Err(format!("webhook returned HTTP {}", status))
    }
}

// ─── Function dispatch (runtime) ─────────────────────────────────────────────

async fn dispatch_function(
    http: &Client,
    runtime_url: &str,
    config: &Value,
    payload: &Value,
    request_id: &str,
) -> Result<u16, String> {
    let function_id = config
        .get("function_id")
        .and_then(|v| v.as_str())
        .ok_or("function config missing 'function_id'")?;

    let body = serde_json::json!({
        "function_id": function_id,
        "payload": payload,
    });

    let resp = http
        .post(format!("{}/internal/execute", runtime_url))
        .header("x-request-id", request_id)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("function dispatch failed: {}", e))?;

    let status = resp.status().as_u16();
    if status < 400 {
        Ok(status)
    } else {
        Err(format!("function returned HTTP {}", status))
    }
}

// ─── Queue job dispatch ───────────────────────────────────────────────────────

async fn dispatch_queue_job(
    pool: &PgPool,
    config: &Value,
    payload: &Value,
) -> Result<u16, String> {
    let job_type = config
        .get("job_type")
        .and_then(|v| v.as_str())
        .unwrap_or("event_triggered");

    let queue = config
        .get("queue")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    sqlx::query(
        "INSERT INTO fluxbase_internal.job_queue \
             (job_type, queue, payload, status) \
         VALUES ($1, $2, $3, 'pending')",
    )
    .bind(job_type)
    .bind(queue)
    .bind(payload)
    .execute(pool)
    .await
    .map_err(|e| format!("queue insert failed: {}", e))?;

    Ok(200)
}

// ─── HMAC-SHA256 helper ───────────────────────────────────────────────────────

fn hmac_sha256(secret: &str, message: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    type HmacSha256 = Hmac<Sha256>;

    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}
