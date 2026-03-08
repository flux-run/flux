use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::config::{Builder, Credentials, Region};
use aws_sdk_s3::presigning::PresigningConfig;
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use std::env;
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct StorageService {
    client: Client,
    bucket_name: String,
}

impl StorageService {
    pub async fn new() -> Self {
        let endpoint = env::var("S3_ENDPOINT").unwrap_or_else(|_| "http://127.0.0.1:9000".to_string());
        let access_key = env::var("S3_ACCESS_KEY_ID").unwrap_or_else(|_| "minioadmin".to_string());
        let secret_key = env::var("S3_SECRET_ACCESS_KEY").unwrap_or_else(|_| "minioadmin".to_string());
        let bucket_name = env::var("S3_BUCKET_NAME").unwrap_or_else(|_| "fluxbase-bundles".to_string());
        let region_name = env::var("S3_REGION").unwrap_or_else(|_| "us-east-1".to_string());

        let region_provider = RegionProviderChain::first_try(Region::new(region_name));
        let credentials = Credentials::new(access_key, secret_key, None, None, "env");

        let shared_config = aws_config::from_env()
            .region(region_provider)
            .credentials_provider(credentials)
            .endpoint_url(endpoint)
            .load()
            .await;

        let mut config_builder = Builder::from(&shared_config);
        config_builder = config_builder.force_path_style(true); // Required for MinIO/R2

        let s3_config = config_builder.build();
        let client = Client::from_conf(s3_config);

        Self {
            client,
            bucket_name,
        }
    }

    pub async fn put_object(&self, key: &str, body: Vec<u8>, content_type: &str) -> std::result::Result<(), String> {
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
