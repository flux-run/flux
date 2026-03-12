/// Unified error and response types for the Flux API service.
///
/// Error envelope matches §12 of framework.md:
/// ```json
/// {
///   "error":   "CONFLICT",
///   "message": "email already registered",
///   "code":    409
/// }
/// ```
///
/// Success responses are NOT wrapped — handlers return data directly:
/// ```json
/// { "functions": [...] }
/// ```
/// The `ApiResponse<T>` newtype serialises `T` as-is; IntoResponse returns
/// it with HTTP 200.  Use `ApiResponse::created(data)` for 201.
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;

// ── Success wrapper ────────────────────────────────────────────────────────

/// Transparent success wrapper.  `T` serialises as the top-level JSON object.
#[derive(Debug)]
pub struct ApiResponse<T: Serialize> {
    status: StatusCode,
    data:   T,
}

impl<T: Serialize> ApiResponse<T> {
    /// HTTP 200 OK
    pub fn new(data: T) -> Self {
        Self { status: StatusCode::OK, data }
    }
    /// HTTP 201 Created
    pub fn created(data: T) -> Self {
        Self { status: StatusCode::CREATED, data }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        (self.status, Json(self.data)).into_response()
    }
}

// Convenience alias for handler return types.
pub type ApiResult<T> = Result<ApiResponse<T>, ApiError>;

// ── Error type ────────────────────────────────────────────────────────────

/// Framework §12 error codes.
///
/// Each variant matches exactly one HTTP status + machine-readable code.
/// The `message` field is human-readable and shown in CLI/dashboard output.
#[derive(Debug)]
pub struct ApiError {
    pub status:  StatusCode,
    /// Machine-readable code — one of the §12 standard codes.
    pub code:    &'static str,
    pub message: String,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self { status, code, message: message.into() }
    }

    // ── §12 standard constructors ─────────────────────────────────────────

    /// 400 — Failed JSON Schema / Zod validation.
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "INPUT_VALIDATION_ERROR", message)
    }

    /// 401 — Missing or invalid auth token.
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "UNAUTHORIZED", message)
    }

    /// 403 — Auth OK, insufficient permissions.
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "FORBIDDEN", message)
    }

    /// 404 — Resource doesn't exist.
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "NOT_FOUND", message)
    }

    /// 409 — Duplicate / state conflict.
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "CONFLICT", message)
    }

    /// 429 — Too many requests.
    pub fn rate_limited(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, "RATE_LIMITED", message)
    }

    /// 500 — Unhandled exception / internal error.
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "FUNCTION_ERROR", message)
    }

    /// 502 — External dependency call failed.
    pub fn dependency(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_GATEWAY, "DEPENDENCY_ERROR", message)
    }

    /// 504 — Function exceeded timeout.
    pub fn timeout(message: impl Into<String>) -> Self {
        Self::new(StatusCode::GATEWAY_TIMEOUT, "TIMEOUT", message)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        tracing::warn!(
            status = self.status.as_u16(),
            code   = self.code,
            msg    = %self.message,
            "api error",
        );
        let body = serde_json::json!({
            "error":   self.code,
            "message": self.message,
            "code":    self.status.as_u16(),
        });
        (self.status, Json(body)).into_response()
    }
}

// ── Standard From conversions ─────────────────────────────────────────────

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        tracing::error!(db_error = %err, "database error");
        // Propagate unique-violation as CONFLICT so callers can detect it.
        if let sqlx::Error::Database(ref db) = err {
            if db.is_unique_violation() {
                return Self::conflict("conflict_duplicate_key");
            }
        }
        Self::internal("database_error")
    }
}

impl From<String> for ApiError {
    fn from(s: String) -> Self { Self::internal(s) }
}

impl From<&str> for ApiError {
    fn from(s: &str) -> Self { Self::internal(s) }
}
