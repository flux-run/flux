use std::time::Duration;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::{
    config::Region,
    presigning::PresigningConfig,
    Client,
};
use uuid::Uuid;

use crate::engine::error::EngineError;

/// Wraps an AWS S3 client configured for Fluxbase's storage conventions.
///
/// Storage path convention:
///   `{tenant}/{project}/{schema}/{table}/{row_id}/{column}/{uuid}.{ext}`
///
/// This mirrors the design spec so files are cleanly scoped to their owning row.
pub struct FileEngine {
    client: Client,
    bucket: String,
}

impl FileEngine {
    /// Initialise with optional custom endpoint (for MinIO / Localstack dev環境).
    pub async fn new(bucket: String, region: String, endpoint_url: Option<String>) -> Self {
        let region_provider = RegionProviderChain::first_try(Region::new(region))
            .or_default_provider();

        let mut config_loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(region_provider);

        if let Some(ref endpoint) = endpoint_url {
            config_loader = config_loader.endpoint_url(endpoint.clone());
        }

        let aws_cfg = config_loader.load().await;

        let mut s3_builder = aws_sdk_s3::config::Builder::from(&aws_cfg);
        if endpoint_url.is_some() {
            // Required for non-AWS compatible storage (path-style addressing).
            s3_builder = s3_builder.force_path_style(true);
        }

        let client = Client::from_conf(s3_builder.build());
        Self { client, bucket }
    }

    /// Build the canonical S3 object key for a file column value.
    ///
    /// `ext` — file extension without leading dot (e.g. "png", "pdf").
    pub fn object_key(
        schema: &str,
        table: &str,
        row_id: &str,
        column: &str,
        ext: &str,
    ) -> String {
        let file_id = Uuid::new_v4();
        format!(
            "{schema}/{table}/{row_id}/{column}/{file_id}.{ext}",
        )
    }

    /// Generate a presigned PUT URL for a direct client upload.
    /// The caller must PUT the file to this URL within the expiry window.
    ///
    /// Returns `(presigned_url, object_key)` — the key must be stored on the row.
    pub async fn upload_url(
        &self,
        key: &str,
        content_type: Option<&str>,
        expires_in: Option<Duration>,
    ) -> Result<String, EngineError> {
        let ttl = expires_in.unwrap_or(Duration::from_secs(900)); // 15 min default
        let presigning = PresigningConfig::expires_in(ttl)
            .map_err(|e| EngineError::Internal(anyhow::anyhow!("presigning config: {}", e)))?;

        let mut req = self.client.put_object().bucket(&self.bucket).key(key);
        if let Some(ct) = content_type {
            req = req.content_type(ct);
        }

        let presigned = req
            .presigned(presigning)
            .await
            .map_err(|e| EngineError::Internal(anyhow::anyhow!("presigned PUT error: {}", e)))?;

        Ok(presigned.uri().to_string())
    }

    /// Generate a presigned GET URL for a stored file key.
    pub async fn download_url(
        &self,
        key: &str,
        expires_in: Option<Duration>,
    ) -> Result<String, EngineError> {
        let ttl = expires_in.unwrap_or(Duration::from_secs(3600)); // 1 hour default
        let presigning = PresigningConfig::expires_in(ttl)
            .map_err(|e| EngineError::Internal(anyhow::anyhow!("presigning config: {}", e)))?;

        let presigned = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(key)
            .presigned(presigning)
            .await
            .map_err(|e| EngineError::Internal(anyhow::anyhow!("presigned GET error: {}", e)))?;

        Ok(presigned.uri().to_string())
    }

    /// Delete a stored object. Called when the owning row is deleted.
    pub async fn delete_object(&self, key: &str) -> Result<(), EngineError> {
        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(key)
            .send()
            .await
            .map_err(|e| EngineError::Internal(anyhow::anyhow!("S3 delete error: {}", e)))?;
        Ok(())
    }
}
