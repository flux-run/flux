use crate::client::ApiClient;
use reqwest::multipart;
use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::Command;

/// Extract schema metadata embedded in a bundle by the defineFunction SDK.
/// The SDK serialises metadata as a JS object literal — we extract it by
/// running Node.js to `eval` and print the metadata as JSON.
fn extract_bundle_metadata(bundle_path: &Path) -> Option<Value> {
    // Quick extraction: look for the __fluxbase marker + metadata JSON in the bundle
    let bundle_content = fs::read_to_string(bundle_path).ok()?;

    // The SDK bakes metadata as a literal object: { name, description, input_schema, output_schema }
    // We run a tiny Node.js snippet to import the bundle and print its metadata
    let script = format!(
        r#"
import('file://{bundle}').then(m => {{
  const fn = m.default;
  if (fn && fn.__fluxbase) {{
    console.log(JSON.stringify(fn.metadata));
  }} else {{
    console.log('null');
  }}
}}).catch(() => console.log('null'));
"#,
        bundle = bundle_path.display()
    );

    let tmp = std::env::temp_dir().join("fluxbase_meta_extract.mjs");
    std::fs::write(&tmp, script).ok()?;

    let out = Command::new("node")
        .arg(&tmp)
        .output()
        .ok()?;

    // Also try to detect __fluxbase in the bundle text to fall back gracefully
    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if stdout != "null" && !stdout.is_empty() {
            return serde_json::from_str(&stdout).ok();
        }
    }

    // Fallback: detect if it's a framework bundle at all (has __fluxbase marker)
    if bundle_content.contains("__fluxbase") {
        return Some(serde_json::json!({ "name": null, "description": null, "input_schema": null, "output_schema": null }));
    }

    None
}

pub async fn execute(
    arg_name: Option<String>,
    arg_runtime: Option<String>,
) -> anyhow::Result<()> {
    // Read flux.json
    let flux_json_path = Path::new("flux.json");
    if !flux_json_path.exists() {
        anyhow::bail!("Error: 'flux.json' not found. Are you in a function directory?\n       Run 'flux function create <name>' to scaffold one.");
    }

    let flux_json_content = fs::read_to_string(flux_json_path)?;
    let metadata: serde_json::Value = serde_json::from_str(&flux_json_content)?;

    let name = arg_name
        .or_else(|| metadata["name"].as_str().map(String::from))
        .or_else(|| {
            // Derive name from the current directory name
            std::env::current_dir().ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
        })
        .unwrap_or_else(|| "unknown".to_string());

    let runtime = arg_runtime
        .or_else(|| metadata["runtime"].as_str().map(String::from))
        .unwrap_or_else(|| "deno".to_string());

    let entry = metadata["entry"].as_str().unwrap_or("index.ts").to_string();

    // Enforce TypeScript-only
    if !entry.ends_with(".ts") {
        anyhow::bail!(
            "Error: entry '{}' must be a TypeScript file (.ts).\n\
             Fluxbase only supports TypeScript functions. Update your flux.json 'entry' field.",
            entry
        );
    }

    if !Path::new(&entry).exists() {
        anyhow::bail!("Error: Entry file '{}' not found.", entry);
    }

    println!("📦 Bundling '{}' ({})...", name, entry);

    // Bundle with esbuild
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
            "--platform=neutral",
            "--format=iife",
            "--global-name=__fluxbase_fn",
            &format!("--outfile={}", bundle_path.display()),
        ])
        .status();

    match status {
        Ok(st) if st.success() => {}
        Ok(_) => anyhow::bail!("esbuild failed. Ensure 'npx esbuild' is available or run 'npm install' in this directory."),
        Err(_) => anyhow::bail!("Failed to run esbuild via npx. Run 'npm install' to install dependencies."),
    }

    let bundle_content = fs::read(&bundle_path)?;
    println!("   Bundle: {} bytes", bundle_content.len());

    // Extract schema metadata from the bundle
    let fn_metadata = extract_bundle_metadata(&bundle_path);

    // Build multipart form
    let part = multipart::Part::bytes(bundle_content)
        .file_name("bundle.js")
        .mime_str("application/javascript")?;

    let mut form = multipart::Form::new()
        .text("name", name.clone())
        .text("runtime", runtime.clone())
        .part("bundle", part);

    if let Some(ref meta) = fn_metadata {
        if let Some(desc) = meta.get("description").and_then(|d| d.as_str()) {
            form = form.text("description", desc.to_string());
        }
        if let Some(input_schema) = meta.get("input_schema").filter(|v| !v.is_null()) {
            form = form.text("input_schema", input_schema.to_string());
        }
        if let Some(output_schema) = meta.get("output_schema").filter(|v| !v.is_null()) {
            form = form.text("output_schema", output_schema.to_string());
        }
    }

    println!("🚀 Deploying '{}'...", name);
    let client = ApiClient::new().await?;
    let res = client.deploy_function(form).await?;

    println!("\n✅ Function deployed successfully!\n");

    let data = res.get("data").unwrap_or(&res);

    if let Some(url) = data.get("url").and_then(|u| u.as_str()) {
        println!("   URL:          {}", url);
    }
    if let Some(fid) = data.get("function_id").and_then(|u| u.as_str()) {
        println!("   Function ID:  {}", fid);
    }
    if let Some(ver) = data.get("version").and_then(|v| v.as_u64()) {
        println!("   Version:      v{}", ver);
    }
    println!("");
    println!("   Test with:  flux invoke {}", name);

    Ok(())
}
