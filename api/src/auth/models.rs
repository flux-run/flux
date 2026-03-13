use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A platform operator account stored in `flux.platform_users`.
#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct PlatformUser {
    pub id:            Uuid,
    pub username:      String,
    pub email:         String,
    /// Never serialised — password_hash must not leak into API responses.
    #[serde(skip)]
    pub password_hash: String,
    pub role:          String,
    pub tenant_id:     Option<Uuid>,
    pub created_at:    chrono::DateTime<chrono::Utc>,
}

/// Request body for `POST /auth/login`.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub email:    String,
    pub password: String,
}

/// Request body for `POST /auth/users` (admin only).
#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username:  String,
    pub email:     String,
    pub password:  String,
    /// One of: "admin" | "viewer" | "readonly"
    pub role:      String,
    pub tenant_id: Option<Uuid>,
}

/// JWT claims embedded in every session token.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// Subject — platform user UUID.
    pub sub:       String,
    pub email:     String,
    pub role:      String,
    pub tenant_id: Option<String>,
    /// Expiry (Unix timestamp).
    pub exp:       usize,
    /// Issued-at (Unix timestamp).
    pub iat:       usize,
}

/// Subset of user fields returned in API responses (no password hash).
#[derive(Debug, Serialize)]
pub struct UserInfo {
    pub id:        Uuid,
    pub username:  String,
    pub email:     String,
    pub role:      String,
    pub tenant_id: Option<Uuid>,
}

impl From<PlatformUser> for UserInfo {
    fn from(u: PlatformUser) -> Self {
        UserInfo {
            id:        u.id,
            username:  u.username,
            email:     u.email,
            role:      u.role,
            tenant_id: u.tenant_id,
        }
    }
}
