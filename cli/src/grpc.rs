use anyhow::{Context, Result, bail};
use tonic::Request;
use tonic::metadata::MetadataValue;

pub mod pb {
    tonic::include_proto!("flux.internal.v1");
}

pub async fn validate_service_token(url: &str, token: &str) -> Result<String> {
    let endpoint = normalize_grpc_url(url);
    let mut client = pb::internal_auth_service_client::InternalAuthServiceClient::connect(endpoint.clone())
        .await
        .with_context(|| format!("failed to connect to Flux server at {}", endpoint))?;

    let mut request = Request::new(pb::ValidateTokenRequest {});
    request.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {}", token))
            .context("service token contains invalid metadata characters")?,
    );

    let response = client
        .validate_token(request)
        .await
        .context("service token validation failed")?
        .into_inner();

    if !response.ok {
        bail!("service token was rejected by the server");
    }

    Ok(response.auth_mode)
}

pub fn normalize_grpc_url(url: &str) -> String {
    let trimmed = url.trim().trim_end_matches('/');
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    }
}