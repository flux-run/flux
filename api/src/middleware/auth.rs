//! Authentication middleware.
//!
//! ## Three operating modes (checked in order)
//!
//! | Mode | When | Behaviour |
//! |------|------|-----------|
//! | **JWT** | `Authorization: Bearer <jwt>` | Dashboard / user sessions |
//! | **DB API key** | `Authorization: Bearer flux_<key>` or `X-API-Key: flux_<key>` | CLI / service keys created via POST /api-keys |
//! | **Static env key** | `FLUX_API_KEY` env var is set | Simple deployment guard |
//! | **Local** (default) | No env vars, no key | `flux dev` — pass through |
//!
//! In all modes the middleware injects a [`RequestContext`] so downstream
//! handlers that declare `Extension(ctx): Extension<RequestContext>` compile.
//!
//! ## SOLID note (Dependency Inversion)
//! Routes depend on `RequestContext` (abstraction), not on how auth works.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use sha2::{Digest, Sha256};

use crate::auth::service as auth_service;
use crate::types::context::RequestContext;
use crate::AppState;

/// Auth + context middleware — mounted via `from_fn_with_state` on the API router.
pub async fn require_auth(
    State(state): State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    // CORS preflights never need auth.
    if req.method() == axum::http::Method::OPTIONS {
        return next.run(req).await;
    }

    let bearer = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(str::to_owned);

    // Allow SSE clients to pass JWT as ?token= query param (EventSource can't
    // set custom headers in all browsers).
    let bearer = bearer.or_else(|| {
        req.uri().query().and_then(|q| {
            q.split('&').find_map(|pair| {
                let mut it = pair.splitn(2, '=');
                let key = it.next()?;
                let val = it.next()?;
                if key == "token" { Some(val.to_owned()) } else { None }
            })
        })
    });

    // ── 1. Try dashboard JWT first ────────────────────────────────────────
    if let Some(ref token) = bearer {
        if let Some(claims) = auth_service::verify_token(token) {
            // Dashboard JWT is valid — inject context and enforce RBAC.
            // viewer/readonly roles may not mutate state.
            let is_mutating = matches!(
                req.method().as_str(),
                "POST" | "PUT" | "PATCH" | "DELETE"
            );
            if is_mutating && claims.role != "admin" {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "error":   "FORBIDDEN",
                        "message": "Write access requires admin role",
                        "code":    403,
                    })),
                )
                .into_response();
            }

            req.extensions_mut().insert(RequestContext);
            return next.run(req).await;
        }
    }

    // ── 2. FLUX_API_KEY guard (CLI / service-to-service) ──────────────────
    if let Ok(expected) = std::env::var("FLUX_API_KEY") {
        use subtle::ConstantTimeEq;
        let provided = bearer.as_deref().unwrap_or("");
        // Constant-time comparison prevents timing-based enumeration of the key.
        let token_ok: bool = provided.as_bytes().ct_eq(expected.as_bytes()).into();
        if !token_ok {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error":   "UNAUTHORIZED",
                    "message": "Invalid or missing API key. Set Authorization: Bearer <FLUX_API_KEY>",
                    "code":    401,
                })),
            )
            .into_response();
        }
    }

    // ── 2b. DB-stored API key (created via POST /api-keys) ────────────────
    // Keys have the format `flux_<32 lowercase hex chars>`.
    // We SHA-256 hash the raw key and compare against key_hash in flux.api_keys.
    // This runs only when no static FLUX_API_KEY env var is configured, so the
    // two modes are mutually exclusive.
    let raw_key = bearer
        .as_deref()
        .map(|s| s.to_owned())
        .or_else(|| {
            req.headers()
                .get("X-API-Key")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_owned())
        });

    if let Some(ref key) = raw_key {
        if key.starts_with("flux_") && std::env::var("FLUX_API_KEY").is_err() {
            let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
            match sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM flux.api_keys WHERE key_hash = $1 AND revoked_at IS NULL)",
            )
            .bind(&hash)
            .fetch_one(&state.pool)
            .await
            {
                Ok(true) => {
                    // Valid key — fire-and-forget last_used_at update.
                    let pool = state.pool.clone();
                    let h = hash.clone();
                    tokio::spawn(async move {
                        let _ = sqlx::query(
                            "UPDATE flux.api_keys SET last_used_at = now() WHERE key_hash = $1",
                        )
                        .bind(&h)
                        .execute(&pool)
                        .await;
                    });

                    req.extensions_mut().insert(RequestContext);
                    return next.run(req).await;
                }
                Ok(false) => {
                    return (
                        StatusCode::UNAUTHORIZED,
                        Json(serde_json::json!({
                            "error":   "UNAUTHORIZED",
                            "message": "Invalid or revoked API key",
                            "code":    401,
                        })),
                    )
                    .into_response();
                }
                Err(e) => {
                    tracing::error!(error = %e, "api_key DB lookup failed");
                    // Fall through to dev/local mode on DB error rather than hard-failing.
                }
            }
        }
    }

    // ── 3. Dev / local mode — no env vars set, pass through ──────────────
    req.extensions_mut().insert(RequestContext);

    next.run(req).await
}
