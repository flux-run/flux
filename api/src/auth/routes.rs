use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Value};
use uuid::Uuid;
use dashmap::DashMap;
use std::{sync::OnceLock, time::{Duration, Instant}};

use crate::{error::ApiError, AppState};
use super::{
    models::{CreateUserRequest, LoginRequest, UserInfo},
    service,
};

// ── Login rate-limit (in-memory, per-email, resets per process) ───────────────

/// 10 attempts per 15-minute window per email address.
const RATE_LIMIT_MAX: u32     = 10;
const RATE_LIMIT_WINDOW: u64  = 900; // seconds

struct AttemptRecord {
    count:        u32,
    window_start: Instant,
}

static LOGIN_LIMITER: OnceLock<DashMap<String, AttemptRecord>> = OnceLock::new();

/// Returns `true` if the request should be blocked (too many attempts).
fn is_rate_limited(email: &str) -> bool {
    let map = LOGIN_LIMITER.get_or_init(DashMap::new);
    let now = Instant::now();
    let window = Duration::from_secs(RATE_LIMIT_WINDOW);

    let mut entry = map.entry(email.to_lowercase()).or_insert_with(|| AttemptRecord {
        count: 0,
        window_start: now,
    });

    if now.duration_since(entry.window_start) > window {
        entry.count = 0;
        entry.window_start = now;
    }

    entry.count += 1;
    entry.count > RATE_LIMIT_MAX
}

/// Reset the attempt counter for an email on successful login (UX: legitimate
/// users aren't locked out after they recover their password).
fn reset_rate_limit(email: &str) {
    if let Some(map) = LOGIN_LIMITER.get() {
        map.remove(&email.to_lowercase());
    }
}

// ── Helper: extract Bearer JWT from headers ───────────────────────────────────

fn extract_claims(headers: &HeaderMap) -> Result<super::models::Claims, ApiError> {
    let token = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "Missing Authorization header"))?;
    service::verify_token(token)
        .ok_or_else(|| ApiError::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "Invalid or expired session token"))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /auth/setup  — create the very first admin (only if 0 users exist)
// ─────────────────────────────────────────────────────────────────────────────
pub async fn setup(
    State(state): State<AppState>,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<Value>, ApiError> {
    let count = service::count_users(&state.pool).await?;
    if count > 0 {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "SETUP_ALREADY_DONE",
            "Initial setup has already been completed. Use /auth/users to manage accounts.",
        ));
    }
    let mut req = body;
    req.role = "admin".to_string(); // first user is always admin
    let user = service::create_user(&state.pool, req).await?;
    let token = service::create_token(&user)?;
    Ok(Json(json!({
        "token": token,
        "user": UserInfo::from(user),
    })))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /auth/login
// ─────────────────────────────────────────────────────────────────────────────
pub async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<Value>, ApiError> {
    if is_rate_limited(&body.email) {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            "TOO_MANY_REQUESTS",
            "Too many login attempts. Please wait 15 minutes before trying again.",
        ));
    }

    let user = service::find_by_email(&state.pool, &body.email)
        .await?
        .ok_or_else(|| {
            ApiError::new(StatusCode::UNAUTHORIZED, "INVALID_CREDENTIALS", "Invalid email or password")
        })?;

    if !service::verify_password(&body.password, &user.password_hash) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "INVALID_CREDENTIALS",
            "Invalid email or password",
        ));
    }

    reset_rate_limit(&body.email);
    let token = service::create_token(&user)?;
    Ok(Json(json!({
        "token": token,
        "user": UserInfo::from(user),
    })))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /auth/logout  — stateless JWT: client discards token
// ─────────────────────────────────────────────────────────────────────────────
pub async fn logout() -> Json<Value> {
    Json(json!({ "success": true }))
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /auth/me  — returns current user info
// ─────────────────────────────────────────────────────────────────────────────
pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let claims = extract_claims(&headers)?;
    let id = Uuid::parse_str(&claims.sub)
        .map_err(|_| ApiError::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", "Malformed token"))?;
    let user = service::find_by_id(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "NOT_FOUND", "User no longer exists"))?;
    Ok(Json(json!({ "user": UserInfo::from(user) })))
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /auth/users  — list all platform users (admin only)
// ─────────────────────────────────────────────────────────────────────────────
pub async fn list_users(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let claims = extract_claims(&headers)?;
    require_admin(&claims)?;
    let users: Vec<UserInfo> = service::list_users(&state.pool)
        .await?
        .into_iter()
        .map(UserInfo::from)
        .collect();
    Ok(Json(json!({ "users": users })))
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /auth/users  — create a new platform user (admin only)
// ─────────────────────────────────────────────────────────────────────────────
pub async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<CreateUserRequest>,
) -> Result<Json<Value>, ApiError> {
    let claims = extract_claims(&headers)?;
    require_admin(&claims)?;
    let user = service::create_user(&state.pool, body).await?;
    Ok(Json(json!({ "user": UserInfo::from(user) })))
}

// ─────────────────────────────────────────────────────────────────────────────
// DELETE /auth/users/:id  — remove a platform user (admin only)
// ─────────────────────────────────────────────────────────────────────────────
pub async fn delete_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let claims = extract_claims(&headers)?;
    require_admin(&claims)?;
    let deleted = service::delete_user(&state.pool, id).await?;
    if deleted {
        Ok(Json(json!({ "success": true })))
    } else {
        Err(ApiError::new(StatusCode::NOT_FOUND, "NOT_FOUND", "User not found"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn require_admin(claims: &super::models::Claims) -> Result<(), ApiError> {
    if claims.role != "admin" {
        Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "FORBIDDEN",
            "Admin role required for this operation",
        ))
    } else {
        Ok(())
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /auth/status  — public, returns user_count so CLI can detect first-run
// ─────────────────────────────────────────────────────────────────────────────
pub async fn status(
    State(state): State<AppState>,
) -> Result<Json<Value>, ApiError> {
    let count = service::count_users(&state.pool).await?;
    Ok(Json(json!({
        "setup_complete": count > 0,
        "user_count": count,
    })))
}
