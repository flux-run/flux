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

    // ── Optional bearer-token guard ──────────────────────────────────────
    //
    // If `FLUX_API_KEY` is set the request MUST carry:
    //   Authorization: Bearer <FLUX_API_KEY value>
    //
    // If the variable is absent we are in local / dev mode and skip the check.
    if let Ok(expected) = std::env::var("FLUX_API_KEY") {
        let provided = req
            .headers()
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.strip_prefix("Bearer "))
            .unwrap_or("");

        if provided != expected {
            return (
                StatusCode::UNAUTHORIZED,
                Json(serde_json::json!({
                    "error":   "UNAUTHORIZED",
                    "message": "Invalid or missing API key. Set Authorization: Bearer <FLUX_API_KEY>",
                    "code":    401u16,
                })),
            )
            .into_response();
        }
    }

    // ── Project context ──────────────────────────────────────────────────
    //
    // The CLI sends `X-Flux-Project: <uuid>` on every request.
    // If absent (e.g. unauthenticated dashboard calls) fall back to the
    // local default so handlers always receive a valid Uuid.
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
