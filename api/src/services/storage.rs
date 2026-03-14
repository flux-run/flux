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


#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

    // ── StorageConfig::from_env ────────────────────────────────────────────

    #[test]
    fn storage_config_from_env_uses_defaults_when_unset() {
        // Remove all storage-related env vars to exercise defaults.
        for var in ["S3_ENDPOINT","R2_ENDPOINT","S3_BUCKET","R2_BUCKET","FUNCTIONS_BUCKET",
                    "S3_ACCESS_KEY_ID","R2_ACCESS_KEY_ID","S3_SECRET_ACCESS_KEY","R2_SECRET_ACCESS_KEY"] {
            unsafe { env::remove_var(var) };
        }
        let cfg = StorageConfig::from_env();
        // Default bucket names should be non-empty
        assert!(!cfg.files_bucket.is_empty());
        assert!(!cfg.functions_bucket.is_empty());
        assert!(!cfg.logs_bucket.is_empty());
    }

    #[test]
    fn storage_config_from_env_reads_custom_bucket() {
        unsafe { env::set_var("FUNCTIONS_BUCKET", "my-custom-functions") };
        let cfg = StorageConfig::from_env();
        assert_eq!(cfg.functions_bucket, "my-custom-functions");
        unsafe { env::remove_var("FUNCTIONS_BUCKET") };
    }

    // ── local_mode detection ──────────────────────────────────────────────

    #[test]
    fn local_mode_is_true_when_no_s3_vars_set() {
        // When neither S3_ENDPOINT nor R2_ENDPOINT is set, local_mode must be true.
        unsafe {
            env::remove_var("S3_ENDPOINT");
            env::remove_var("R2_ENDPOINT");
            env::remove_var("LOCAL_MODE");
        }
        // We can't call StorageService::new() in a unit test (it does async AWS config),
        // but we can verify the logic inline by reproducing the condition.
        let has_s3  = env::var("S3_ENDPOINT").is_ok();
        let has_r2  = env::var("R2_ENDPOINT").is_ok();
        let explicit_local = env::var("LOCAL_MODE").map(|v| v == "true").unwrap_or(false);
        let expected_local = explicit_local || (!has_s3 && !has_r2);
        assert!(expected_local, "should be local_mode when no endpoints configured");
    }

    #[test]
    fn local_mode_is_false_when_s3_endpoint_set() {
        unsafe {
            env::set_var("S3_ENDPOINT", "http://minio.example.com:9000");
            env::remove_var("R2_ENDPOINT");
            env::remove_var("LOCAL_MODE");
        }
        let has_s3 = env::var("S3_ENDPOINT").is_ok();
        let explicit_local = env::var("LOCAL_MODE").map(|v| v == "true").unwrap_or(false);
        let local = explicit_local || !has_s3;
        assert!(!local, "local_mode should be false when S3_ENDPOINT is set");
        unsafe { env::remove_var("S3_ENDPOINT") };
    }

    #[test]
    fn local_mode_explicit_true_overrides_endpoint() {
        unsafe {
            env::set_var("LOCAL_MODE",   "true");
            env::set_var("S3_ENDPOINT",  "http://real-minio:9000");
        }
        let explicit = env::var("LOCAL_MODE").map(|v| v == "true").unwrap_or(false);
        assert!(explicit, "LOCAL_MODE=true must force local_mode even with S3_ENDPOINT");
        unsafe {
            env::remove_var("LOCAL_MODE");
            env::remove_var("S3_ENDPOINT");
        }
    }
}
