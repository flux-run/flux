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
    crate::middleware::require_secret(
        "FLUX_JWT_SECRET",
        "flux-dev-jwt-secret-change-in-prod",
        "JWT signing secret (FLUX_JWT_SECRET)",
    )
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

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    // ── Password helpers ──────────────────────────────────────────────────

    #[test]
    fn hash_and_verify_password_roundtrip() {
        let pw = "correct-horse-battery-staple";
        let hash = hash_password(pw).expect("hash failed");
        assert!(verify_password(pw, &hash));
    }

    #[test]
    fn wrong_password_does_not_verify() {
        let hash = hash_password("right").unwrap();
        assert!(!verify_password("wrong", &hash));
    }

    #[test]
    fn empty_password_roundtrip() {
        let hash = hash_password("").unwrap();
        assert!(verify_password("", &hash));
        assert!(!verify_password("notempty", &hash));
    }

    #[test]
    fn long_password_roundtrip() {
        let pw = "p".repeat(256);
        let hash = hash_password(&pw).unwrap();
        assert!(verify_password(&pw, &hash));
    }

    #[test]
    fn verify_with_garbage_hash_returns_false() {
        assert!(!verify_password("any", "not_a_valid_hash_string"));
    }

    #[test]
    fn two_hashes_of_same_password_are_different() {
        let h1 = hash_password("pw").unwrap();
        let h2 = hash_password("pw").unwrap();
        assert_ne!(h1, h2, "salt must be randomised per call");
    }

    // ── JWT helpers ───────────────────────────────────────────────────────

    fn dummy_user() -> PlatformUser {
        PlatformUser {
            id:        Uuid::new_v4(),
            username:  "testuser".to_string(),
            email:     "test@example.com".to_string(),
            password_hash: "x".to_string(),
            role:      "admin".to_string(),
            tenant_id: Some(Uuid::new_v4()),
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn create_and_verify_token_roundtrip() {
        let user  = dummy_user();
        let token = create_token(&user).expect("token creation failed");
        let claims = verify_token(&token).expect("token verification failed");
        assert_eq!(claims.email, user.email);
        assert_eq!(claims.sub,   user.id.to_string());
        assert_eq!(claims.role,  user.role);
    }

    #[test]
    fn verify_token_returns_none_for_garbage() {
        assert!(verify_token("this.is.not.a.jwt").is_none());
    }

    #[test]
    fn verify_token_returns_none_for_empty_string() {
        assert!(verify_token("").is_none());
    }

    #[test]
    fn verify_token_returns_none_for_wrong_secret() {
        let user  = dummy_user();
        let token = create_token(&user).unwrap();
        // Tamper: flip last char of the signature
        let parts: Vec<&str> = token.rsplitn(2, '.').collect();
        let bad = format!("{}.XXXXX", parts[1]);
        assert!(verify_token(&bad).is_none());
    }

    #[test]
    fn token_claims_contain_tenant_id() {
        let user   = dummy_user();
        let token  = create_token(&user).unwrap();
        let claims = verify_token(&token).unwrap();
        assert_eq!(
            claims.tenant_id,
            user.tenant_id.map(|t| t.to_string()),
        );
    }

    #[test]
    fn token_is_non_empty_string() {
        let user  = dummy_user();
        let token = create_token(&user).unwrap();
        assert!(!token.is_empty());
        // JWT has exactly three dot-separated segments
        assert_eq!(token.split('.').count(), 3);
    }
}
