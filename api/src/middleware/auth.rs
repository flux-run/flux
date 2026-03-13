//! Authentication + project-context middleware.
//!
//! ## Two operating modes
//!
//! | Mode | When | Behaviour |
//! |------|------|-----------|
//! | **Local** (default) | `FLUX_API_KEY` env var is **not** set | All requests pass through — optimised for `flux dev` |
//! | **Protected** | `FLUX_API_KEY` is set | `Authorization: Bearer <key>` checked on every request |
//!
//! In both modes the middleware:
//!   1. Reads the optional `X-Flux-Project` header (UUID) and uses it as `project_id`.
//!   2. Falls back to `AppState::local_project_id` when the header is absent.
//!   3. Injects a fully-populated [`RequestContext`] so downstream handlers
//!      never need to deal with `Option<Uuid>`.
//!
//! ## SOLID note (Dependency Inversion)
//! Routes depend on `RequestContext` (abstraction), not on how auth works.
//! Swapping from local-mode to external auth = change this one middleware.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use uuid::Uuid;

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

            // Prefer tenant from JWT claims, fall back to app default.
            let tenant_id = claims
                .tenant_id
                .as_deref()
                .and_then(|t| Uuid::parse_str(t).ok())
                .unwrap_or(state.local_tenant_id);

            let project_id: Uuid = req
                .headers()
                .get("X-Flux-Project")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| Uuid::parse_str(s).ok())
                .unwrap_or(state.local_project_id);

            req.extensions_mut().insert(RequestContext { project_id, tenant_id });
            return next.run(req).await;
        }
    }

    // ── 2. FLUX_API_KEY guard (CLI / service-to-service) ──────────────────
    if let Ok(expected) = std::env::var("FLUX_API_KEY") {
        let provided = bearer.as_deref().unwrap_or("");
        if provided != expected {
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

    // ── 3. Dev / local mode — no env vars set, pass through ──────────────
    let project_id: Uuid = req
        .headers()
        .get("X-Flux-Project")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .unwrap_or(state.local_project_id);

    req.extensions_mut().insert(RequestContext {
        project_id,
        tenant_id: state.local_tenant_id,
    });

    next.run(req).await
}
