use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("access denied: no policy permits role '{role}' to {operation} on '{table}'")]
    AccessDenied {
        role: String,
        table: String,
        operation: String,
    },

    #[error("invalid identifier '{0}': only [a-zA-Z0-9_] allowed, max 63 chars")]
    InvalidIdentifier(String),

    #[error("database '{0}' not found for this project")]
    DatabaseNotFound(String),

    #[error("unsupported operation '{0}': expected select | insert | update | delete")]
    UnsupportedOperation(String),

    #[error("missing required field: {0}")]
    MissingField(String),

    #[error("query too complex: score {score} exceeds limit {limit}")]
    QueryTooComplex { score: u64, limit: u64 },

    #[error("query nesting too deep: depth {depth} exceeds limit {limit}")]
    NestDepthExceeded { depth: usize, limit: usize },

    #[error("query timed out")]
    QueryTimeout,

    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("internal error: {0}")]
    Internal(#[from] anyhow::Error),
}

impl EngineError {
    pub fn status(&self) -> axum::http::StatusCode {
        use axum::http::StatusCode;
        match self {
            Self::AccessDenied { .. }        => StatusCode::FORBIDDEN,
            Self::InvalidIdentifier(_)         => StatusCode::BAD_REQUEST,
            Self::DatabaseNotFound(_)          => StatusCode::NOT_FOUND,
            Self::UnsupportedOperation(_)      => StatusCode::BAD_REQUEST,
            Self::MissingField(_)              => StatusCode::BAD_REQUEST,
            Self::QueryTooComplex { .. }       => StatusCode::BAD_REQUEST,
            Self::NestDepthExceeded { .. }     => StatusCode::BAD_REQUEST,
            Self::QueryTimeout                 => StatusCode::REQUEST_TIMEOUT,
            Self::Db(_) | Self::Internal(_)    => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl axum::response::IntoResponse for EngineError {
    fn into_response(self) -> axum::response::Response {
        let status = self.status();
        let body = serde_json::json!({ "error": self.to_string() });
        (status, axum::Json(body)).into_response()
    }
}
