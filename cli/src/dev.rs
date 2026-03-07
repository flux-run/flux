use std::fs;
use std::path::Path;
use std::process::Command;

pub async fn execute() -> anyhow::Result<()> {
    let flux_json_path = Path::new("flux.json");
    if !flux_json_path.exists() {
        anyhow::bail!("Error: 'flux.json' not found. Are you currently in a function directory?");
    }

    let meta: serde_json::Value = serde_json::from_str(&fs::read_to_string(flux_json_path)?)?;
    let entry = meta["entry"].as_str().unwrap_or("index.ts");

    println!("Bundling {} for local dev...", entry);

    let out_dir = Path::new("dist");
    fs::create_dir_all(out_dir)?;
    let bundle_path = out_dir.join("bundle.js");

    let status = Command::new("npx")
        .args(&[
            "esbuild",
            entry,
            "--bundle",
            "--format=esm",
            &format!("--outfile={}", bundle_path.display()),
        ])
        .status()?;

    if !status.success() {
        anyhow::bail!("Esbuild bundling failed.");
    }

    let simulator_code = r#"
import handler from './dist/bundle.js';

const secretCache = new Map();
try {
  const envText = await Deno.readTextFile('.fluxbase/.env');
  envText.split('\n').forEach(line => {
    const [k, ...v] = line.split('=');
    if (k && v.length) secretCache.set(k.trim(), v.join('=').trim());
  });
} catch (e) { /* ignore if .fluxbase/.env missing */ }

Deno.serve({ port: 8787 }, async (req) => {
    let payload = {};
    try {
        if (req.method === "POST" || req.method === "PUT") {
            payload = await req.json();
        }
    } catch (e) {}

    const ctx = {
        payload,
        env: Deno.env.toObject(),
        secrets: {
            get: (key) => secretCache.get(key) || Deno.env.get(key)
        },
        log: (...args) => console.log("[flux]", ...args)
    };

    console.log(`[REQ] ${req.method} ${req.url}`);

    try {
        const result = await handler(ctx);
        return new Response(JSON.stringify(result), {
            headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" }
        });
    } catch (err) {
        console.error("Function Error:", err);
        return new Response(JSON.stringify({ error: err.message }), {
            status: 500,
            headers: { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" }
        });
    }
});
"#;

    fs::write(".fluxbase_dev_server.js", simulator_code)?;
    
    println!("\nStarting local simulator on http://localhost:8787\n");
    
    let mut child = Command::new("deno")
        .args(&["run", "-A", ".fluxbase_dev_server.js"])
        .spawn()?;

    child.wait()?;

    Ok(())
}
