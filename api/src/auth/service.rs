use argon2::{
    Argon2, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{rand_core::OsRng, SaltString},
};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::ApiError;
use super::models::{Claims, CreateUserRequest, PlatformUser};

// ── JWT secret ────────────────────────────────────────────────────────────────

fn jwt_secret() -> String {
    std::env::var("FLUX_JWT_SECRET")
        .unwrap_or_else(|_| "flux-dev-jwt-secret-change-in-prod".to_string())
}

// ── Password helpers ──────────────────────────────────────────────────────────

/// Hash a plain-text password with Argon2id.
pub fn hash_password(password: &str) -> Result<String, ApiError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| ApiError::internal(format!("Password hashing failed: {e}")))
}

/// Returns `true` when `password` matches the stored Argon2 `hash`.
pub fn verify_password(password: &str, hash: &str) -> bool {
    match PasswordHash::new(hash) {
        Ok(parsed) => Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok(),
        Err(_) => false,
    }
}

// ── JWT helpers ───────────────────────────────────────────────────────────────

/// Issue a 7-day HS256 JWT for a platform user.
pub fn create_token(user: &PlatformUser) -> Result<String, ApiError> {
    let now = chrono::Utc::now();
    let exp = (now + chrono::Duration::days(7)).timestamp() as usize;
    let claims = Claims {
        sub:       user.id.to_string(),
        email:     user.email.clone(),
        role:      user.role.clone(),
        tenant_id: user.tenant_id.map(|t| t.to_string()),
        exp,
        iat:       now.timestamp() as usize,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(jwt_secret().as_bytes()),
    )
    .map_err(|e| ApiError::internal(format!("JWT signing failed: {e}")))
}

/// Validate a JWT and return its claims, or `None` if invalid/expired.
pub fn verify_token(token: &str) -> Option<Claims> {
    let validation = Validation::new(Algorithm::HS256);
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(jwt_secret().as_bytes()),
        &validation,
    )
    .map(|d| d.claims)
    .ok()
}

// ── DB helpers ────────────────────────────────────────────────────────────────

pub async fn count_users(pool: &PgPool) -> Result<i64, ApiError> {
    let row: (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM flux.platform_users"
    )
    .fetch_one(pool)
    .await
    .map_err(|e| ApiError::internal(format!("DB error: {e}")))?;
    Ok(row.0)
}

pub async fn find_by_email(pool: &PgPool, email: &str) -> Result<Option<PlatformUser>, ApiError> {
    sqlx::query_as(
        "SELECT id, username, email, password_hash, role, tenant_id, created_at
         FROM flux.platform_users WHERE email = $1",
    )
    .bind(email)
    .fetch_optional(pool)
    .await
    .map_err(|e| ApiError::internal(format!("DB error: {e}")))
}

pub async fn find_by_id(pool: &PgPool, id: Uuid) -> Result<Option<PlatformUser>, ApiError> {
    sqlx::query_as(
        "SELECT id, username, email, password_hash, role, tenant_id, created_at
         FROM flux.platform_users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await
    .map_err(|e| ApiError::internal(format!("DB error: {e}")))
}

pub async fn create_user(pool: &PgPool, req: CreateUserRequest) -> Result<PlatformUser, ApiError> {
    if !["admin", "viewer", "readonly"].contains(&req.role.as_str()) {
        return Err(ApiError::bad_request(format!(
            "Invalid role '{}'. Must be one of: admin, viewer, readonly",
            req.role
        )));
    }
    let password_hash = hash_password(&req.password)?;
    sqlx::query_as(
        "INSERT INTO flux.platform_users (username, email, password_hash, role, tenant_id)
         VALUES ($1, $2, $3, $4, $5)
         RETURNING id, username, email, password_hash, role, tenant_id, created_at",
    )
    .bind(req.username)
    .bind(req.email)
    .bind(password_hash)
    .bind(req.role)
    .bind(req.tenant_id)
    .fetch_one(pool)
    .await
    .map_err(|e| {
        let msg = e.to_string();
        if msg.contains("unique") {
            ApiError::bad_request("A user with that email or username already exists")
        } else {
            ApiError::internal(format!("Create user failed: {e}"))
        }
    })
}

pub async fn list_users(pool: &PgPool) -> Result<Vec<PlatformUser>, ApiError> {
    sqlx::query_as(
        "SELECT id, username, email, password_hash, role, tenant_id, created_at
         FROM flux.platform_users ORDER BY created_at",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| ApiError::internal(format!("DB error: {e}")))
}

pub async fn delete_user(pool: &PgPool, id: Uuid) -> Result<bool, ApiError> {
    sqlx::query("DELETE FROM flux.platform_users WHERE id = $1")
        .bind(id)
        .execute(pool)
        .await
        .map(|r| r.rows_affected() > 0)
        .map_err(|e| ApiError::internal(format!("DB error: {e}")))
}
