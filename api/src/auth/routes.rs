use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::{error::ApiError, AppState};
use super::{
    models::{CreateUserRequest, LoginRequest, UserInfo},
    service,
};

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
