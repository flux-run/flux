use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::config::{Builder, Credentials, Region};
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use std::env;
use std::time::Duration;

// ─── Shared storage configuration ─────────────────────────────────────────────
//
// Each bucket has a single, well-scoped responsibility:
//
//   FILES_BUCKET      (fluxbase-files)     — user / app file uploads
//   FUNCTIONS_BUCKET  (fluxbase-functions) — function bundles + deployment artifacts
//   LOGS_BUCKET       (fluxbase-logs)      — archived platform logs (NDJSON.gz)
//
// The env var precedence for each bucket is:
//   1. The dedicated var (FILES_BUCKET, FUNCTIONS_BUCKET, LOGS_BUCKET)
//   2. Legacy / compat aliases (S3_BUCKET, R2_BUCKET …)
//   3. Hard-coded default (safe for local dev)

#[derive(Clone, Debug)]
pub struct StorageConfig {
    pub files_bucket:     String,
    pub functions_bucket: String,
    pub logs_bucket:      String,
}

impl StorageConfig {
    pub fn from_env() -> Self {
        // FILES_BUCKET — user uploads (presigned put/get via data-engine)
        let files_bucket = env::var("FILES_BUCKET")
            .unwrap_or_else(|_| "fluxbase-files".to_string());

        // FUNCTIONS_BUCKET — bundle storage (api deploy + runtime fetch)
        let functions_bucket = env::var("FUNCTIONS_BUCKET")
            .or_else(|_| env::var("S3_BUCKET"))
            .or_else(|_| env::var("R2_BUCKET"))
            .unwrap_or_else(|_| "fluxbase-functions".to_string());

        // LOGS_BUCKET — log archiver (api/src/logs/archiver.rs)
        let logs_bucket = env::var("LOGS_BUCKET")
            .or_else(|_| env::var("LOG_BUCKET"))
            .unwrap_or_else(|_| "fluxbase-logs".to_string());

        Self { files_bucket, functions_bucket, logs_bucket }
    }
}

// ─── StorageService (function bundle storage) ─────────────────────────────────
//
// Used exclusively for storing and fetching function bundles / deployment
// artifacts.  Always talks to FUNCTIONS_BUCKET (fluxbase-functions).

#[derive(Clone, Debug)]
pub struct StorageService {
    client:      Client,
    bucket_name: String,
    /// When true (LOCAL_MODE / no S3 configured), skip all S3 operations.
    /// Bundles are stored inline in the `deployments.bundle_code` column.
    pub local_mode: bool,
}

impl StorageService {
    pub async fn new() -> Self {
        // Credentials — prefer the S3_* naming that matches env.yaml production values.
        let endpoint   = env::var("S3_ENDPOINT")
            .or_else(|_| env::var("R2_ENDPOINT"))
            .unwrap_or_else(|_| "http://127.0.0.1:9000".to_string());
        let access_key = env::var("S3_ACCESS_KEY_ID")
            .or_else(|_| env::var("R2_ACCESS_KEY_ID"))
            .unwrap_or_else(|_| "minioadmin".to_string());
        let secret_key = env::var("S3_SECRET_ACCESS_KEY")
            .or_else(|_| env::var("R2_SECRET_ACCESS_KEY"))
            .unwrap_or_else(|_| "minioadmin".to_string());

        // Function bundle bucket — dedicated var first, then legacy aliases.
        let bucket_name = env::var("FUNCTIONS_BUCKET")
            .or_else(|_| env::var("S3_BUCKET"))
            .or_else(|_| env::var("R2_BUCKET"))
            .unwrap_or_else(|_| "fluxbase-functions".to_string());

        // In LOCAL_MODE (or when S3 is not explicitly configured) skip S3.
        // Bundles are stored inline in deployments.bundle_code so the runtime
        // can serve them without any object storage dependency.
        let local_mode = env::var("LOCAL_MODE").map(|v| v == "true").unwrap_or(false)
            || (env::var("S3_ENDPOINT").is_err() && env::var("R2_ENDPOINT").is_err());

        let region_provider = RegionProviderChain::first_try(Region::new("auto"));
        let credentials = Credentials::new(access_key, secret_key, None, None, "env");

        let shared_config = aws_config::from_env()
            .region(region_provider)
            .credentials_provider(credentials)
            .endpoint_url(endpoint)
            .load()
            .await;

        let mut config_builder = Builder::from(&shared_config);
        config_builder = config_builder.force_path_style(true);

        let s3_config = config_builder.build();
        let client = Client::from_conf(s3_config);

        if local_mode {
            tracing::info!("StorageService — local mode (bundles stored inline in DB)");
        } else {
            tracing::info!("StorageService initialised — functions bucket: {}", bucket_name);
        }

        Self { client, bucket_name, local_mode }
    }

    pub async fn put_object(&self, key: &str, body: Vec<u8>, content_type: &str) -> std::result::Result<(), String> {
        if self.local_mode {
            // Bundle already persisted in deployments.bundle_code — skip S3.
            return Ok(());
        }

        let stream = ByteStream::from(body);

        self.client
            .put_object()
            .bucket(&self.bucket_name)
            .key(key)
            .body(stream)
            .content_type(content_type)
            .send()
            .await
            .map_err(|e| format!("Failed to push to storage: {}", e))?;

        Ok(())
    }

    pub async fn presigned_get_object(&self, key: &str, expires_in: Duration) -> std::result::Result<String, String> {
        let presigning_config = PresigningConfig::expires_in(expires_in)
            .map_err(|e| format!("Failed to construct presigning config: {}", e))?;

        let presigned_request = self.client
            .get_object()
            .bucket(&self.bucket_name)
            .key(key)
            .presigned(presigning_config)
            .await
            .map_err(|e| format!("Failed to generate presigned URL: {}", e))?;

        Ok(presigned_request.uri().to_string())
    }
}

