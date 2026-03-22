use anyhow::{Context, Result};
use clap::Args;
use std::env;


use crate::project::{read_deno_config, write_deno_config, DenoConfig};

#[derive(Debug, Args)]
pub struct AddArgs {
    /// Packages to add to the project (e.g., hono, zod, drizzle-orm)
    pub packages: Vec<String>,
}

pub async fn execute(args: AddArgs) -> Result<()> {
    let cwd = env::current_dir().context("failed to read current directory")?;
    
    // Find deno.json in ancestors, or default to cwd/deno.json
    let mut config_path = None;
    for ancestor in cwd.ancestors() {
        let p = ancestor.join("deno.json");
        if p.is_file() {
            config_path = Some(p);
            break;
        }
    }
    
    let config_path = config_path.unwrap_or_else(|| cwd.join("deno.json"));
    
    let mut config = if config_path.exists() {
        read_deno_config(&config_path)?
    } else {
        DenoConfig::default()
    };
    
    let mut imports = config.imports.unwrap_or_default();
    
    for pkg in args.packages {
        // Split name from version if present (e.g., hono@4.0.0)
        let (name, specifier) = if let Some((n, v)) = pkg.split_once('@') {
            (n, format!("npm:{}@{}", n, v))
        } else if pkg.starts_with("npm:") || pkg.starts_with("https://") {
            // Use as is if it looks like a full specifier
            // But we need a name for the key. 
            // For now, let's assume if it starts with npm: or https: we might need to parse it better.
            // Simple heuristic for now:
            let name = pkg.split('/').last().unwrap_or(&pkg).split('@').next().unwrap_or(&pkg);
            (name, pkg.clone())
        } else {
            (pkg.as_str(), format!("npm:{}", pkg))
        };
        
        println!("adding   {} as {}", name, specifier);
        imports.insert(name.to_string(), specifier);
    }
    
    config.imports = Some(imports);
    write_deno_config(&config_path, &config)?;
    
    println!("updated  {}", config_path.display());
    
    Ok(())
}
