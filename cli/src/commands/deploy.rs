use std::path::PathBuf;
use crate::api::client::ApiClient;
use serde_json::json;

pub async fn execute() {
    println!("Deploying Fluxbase Function Bundle...");

    // Validate a typical `flux.toml` deployment config locally
    // In reality this parses `name`, `runtime`, and `entrypoint`
    let _function_dir = std::env::current_dir().unwrap_or(PathBuf::from("."));

    // Simulating the packaging process here...
    // Bundling `index.ts` -> `.tar.gz` and uploading to S3
    // E.g.: upload_bundle(&function_dir).await;
    
    let mock_storage_key = format!("bundles/{}", uuid::Uuid::new_v4());
    
    let client = match ApiClient::new().await {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Auth Error: {}", e);
            std::process::exit(1);
        }
    };

    let payload = json!({
        "name": "send-email",
        "runtime": "deno",
        "entrypoint": "index.ts",
        "storage_key": mock_storage_key,
        "checksum": "mock_sha256_hash",
    });

    match client.deploy_function(payload).await {
        Ok(res) => println!("Deployment Successful! \nResponse: {:#?}", res),
        Err(e) => eprintln!("Deployment Failed: {}", e),
    }
}
