use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use serde_json::json;

#[derive(Debug, Serialize)]
pub struct ApiResponse<T: Serialize> {
    pub success: bool,
    pub data: T,
}

impl<T: Serialize> ApiResponse<T> {
    pub fn new(data: T) -> Self {
        Self { success: true, data }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        Json(self).into_response()
    }
}

#[derive(Debug, Serialize)]
pub struct ApiErrorData {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct ApiErrorResponse {
    pub success: bool,
    pub error: ApiErrorData,
}

#[derive(Debug)]
pub struct ApiError(pub StatusCode, pub String, pub String);

impl ApiError {
    pub fn new(status: StatusCode, code: &str, message: &str) -> Self {
        Self(status, code.to_string(), message.to_string())
    }

    pub fn unauth(msg: &str) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, "unauthorized", msg)
    }

    pub fn forbidden(msg: &str) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", msg)
    }

    pub fn not_found(msg: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", msg)
    }

    pub fn bad_request(msg: &str) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "bad_request", msg)
    }

    pub fn internal(msg: &str) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, "internal_error", msg)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code, message) = (self.0, self.1, self.2);
        
        let payload = ApiErrorResponse {
            success: false,
            error: ApiErrorData { code, message },
        };
        
        (status, Json(payload)).into_response()
    }
}

// Convenient conversion from sqlx::Error to ApiError
impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        ApiError::internal(&format!("Database error: {}", err))
    }
}

// Generic string/anyhow errors map to internal errors
impl From<String> for ApiError {
    fn from(err: String) -> Self {
        ApiError::internal(&err)
    }
}

impl From<&str> for ApiError {
    fn from(err: &str) -> Self {
        ApiError::internal(err)
    }
}
