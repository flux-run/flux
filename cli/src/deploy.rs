use crate::client::ApiClient;
use reqwest::multipart;

use std::fs;
use std::path::Path;
use std::process::Command;

pub async fn execute() -> anyhow::Result<()> {
    // 1. Read flux.json
    let flux_json_path = Path::new("flux.json");
    if !flux_json_path.exists() {
        anyhow::bail!("Error: 'flux.json' not found. Are you in a function directory?");
    }

    let flux_json_content = fs::read_to_string(flux_json_path)?;
    let metadata: serde_json::Value = serde_json::from_str(&flux_json_content)?;

    let name = metadata["name"].as_str().unwrap_or("unknown").to_string();
    let runtime = metadata["runtime"].as_str().unwrap_or("deno").to_string();
    let entry = metadata["entry"].as_str().unwrap_or("index.ts").to_string();

    if !Path::new(&entry).exists() {
        anyhow::bail!("Error: Entry file '{}' not found.", entry);
    }

    println!("Bundling function '{}'...", name);

    // 2. Bundle function
    let out_dir = Path::new("dist");
    if !out_dir.exists() {
        fs::create_dir_all(out_dir)?;
    }
    
    let bundle_path = out_dir.join("bundle.js");

    let status = Command::new("npx")
        .args(&[
            "esbuild",
            &entry,
            "--bundle",
            "--format=esm",
            &format!("--outfile={}", bundle_path.display()),
        ])
        .status();

    if let Ok(st) = status {
        if !st.success() {
            anyhow::bail!("Esbuild failed. Ensure 'npx esbuild' is available or configure your package.json.");
        }
    } else {
        anyhow::bail!("Failed to execute esbuild via npx. Please install esbuild globally or in your project.");
    }

    // 3. Compress bundle (simulating zip/buffer for multipart)
    let bundle_content = fs::read(&bundle_path)?;

    // 4. Upload to control plane
    println!("Deploying function to Control Plane...");
    let client = ApiClient::new().await?;

    let part = multipart::Part::bytes(bundle_content)
        .file_name("bundle.js")
        .mime_str("application/javascript")?;

    let form = multipart::Form::new()
        .text("name", name.clone())
        .text("runtime", runtime.clone())
        .part("bundle", part);

    let res = client.deploy_function(form).await?;

    println!("\nFunction deployed successfully ✅\n");
    if let Some(url) = res.get("url").and_then(|u| u.as_str()) {
        println!("URL:\n{}", url);
    } else {
        // Fallback display if API doesn't return URL
        println!("Ready to invoke: flux invoke {}", name);
    }

    Ok(())
}
