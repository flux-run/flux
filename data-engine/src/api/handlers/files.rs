use axum::{extract::State, http::HeaderMap, Json};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use crate::{
    engine::{auth_context::AuthContext, error::EngineError},
    file_engine::FileEngine,
    router::db_router::validate_identifier,
    state::AppState,
};

#[derive(Debug, Deserialize)]
pub struct UploadUrlRequest {
    /// Logical database name (used in the object key path).
    pub database: String,
    pub table: String,
    /// The existing row's identifier (arbitrary string; typically the PK value).
    pub row_id: String,
    /// Column name the file will be stored in.
    pub column: String,
    /// MIME type, forwarded as Content-Type on the presigned PUT.
    pub content_type: Option<String>,
    /// File extension without dot (e.g. "png"). Defaults to "bin".
    pub extension: Option<String>,
    /// Expiry seconds for the upload URL. Capped at 3600. Default 900.
    pub expires_in: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct UploadUrlResponse {
    /// Presigned PUT URL — the client uploads directly to this.
    pub upload_url: String,
    /// Object key to store on the row after upload completes.
    pub object_key: String,
}

#[derive(Debug, Deserialize)]
pub struct DownloadUrlRequest {
    /// The S3 object key stored on the row.
    pub object_key: String,
    /// Expiry seconds. Capped at 86400. Default 3600.
    pub expires_in: Option<u64>,
}

// ─── POST /files/upload-url ───────────────────────────────────────────────────

pub async fn upload_url(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<UploadUrlRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    validate_identifier(&req.database)?;
    validate_identifier(&req.table)?;
    validate_identifier(&req.column)?;

    let engine = require_file_engine(state.file_engine.as_deref())?;

    let ext = req.extension.as_deref().unwrap_or("bin");
    let key = FileEngine::object_key(
        &auth.tenant_slug,
        &auth.project_slug,
        &req.database,
        &req.table,
        &req.row_id,
        &req.column,
        ext,
    );

    let expires = std::time::Duration::from_secs(
        req.expires_in.unwrap_or(900).min(3600)
    );

    let url = engine
        .upload_url(&key, req.content_type.as_deref(), Some(expires))
        .await?;

    Ok(Json(json!(UploadUrlResponse {
        upload_url: url,
        object_key: key,
    })))
}

// ─── POST /files/download-url ─────────────────────────────────────────────────

pub async fn download_url(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Json(req): Json<DownloadUrlRequest>,
) -> Result<Json<serde_json::Value>, EngineError> {
    let _auth = AuthContext::from_headers(&headers).map_err(EngineError::MissingField)?;

    let engine = require_file_engine(state.file_engine.as_deref())?;

    let expires = std::time::Duration::from_secs(
        req.expires_in.unwrap_or(3600).min(86400)
    );

    let url = engine.download_url(&req.object_key, Some(expires)).await?;

    Ok(Json(json!({ "download_url": url })))
}

// ─── Helper ───────────────────────────────────────────────────────────────────

fn require_file_engine(engine: Option<&FileEngine>) -> Result<&FileEngine, EngineError> {
    engine.ok_or_else(|| {
        EngineError::UnsupportedOperation(
            "file storage is not configured (set FILES_BUCKET env var)".into(),        )
    })
}