use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

pub async fn ensure_binary(name: &str, release: bool) -> Result<PathBuf> {
    // 1. Try finding it in the same directory as the current executable
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let bin_name = if cfg!(windows) {
                format!("{}.exe", name)
            } else {
                name.to_string()
            };
            let bin_path = exe_dir.join(&bin_name);
            if bin_path.exists() {
                return Ok(bin_path);
            }
        }
    }

    // 2. Try finding it in the workspace root (for developers)
    if let Some(workspace_root) = find_workspace_root() {
        if let Some(binary) = find_workspace_binary(&workspace_root, name, release) {
            return Ok(binary);
        }

        // Avoid building if we are clearly not in a dev environment
        // (i.e. if we found a workspace root but it's some other project)
        if is_flux_workspace(&workspace_root) {
            build_workspace_binary(&workspace_root, name, release).await?;
            if let Some(binary) = find_workspace_binary(&workspace_root, name, release) {
                return Ok(binary);
            }
        }
    }

    // 3. Try finding it in PATH
    if let Ok(path) = which::which(name) {
        return Ok(path);
    }

    anyhow::bail!(
        "could not find binary '{}'. Make sure it is installed and in your PATH.",
        name
    )
}

fn find_workspace_root() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let cargo_toml = dir.join("Cargo.toml");
        if cargo_toml.exists() {
            let contents = std::fs::read_to_string(&cargo_toml).ok()?;
            if contents.contains("[workspace]") {
                return Some(dir);
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn is_flux_workspace(workspace_root: &Path) -> bool {
    workspace_root.join("cli").exists() && workspace_root.join("runtime").exists()
}

fn find_workspace_binary(workspace_root: &Path, name: &str, release: bool) -> Option<PathBuf> {
    let bin_name = if cfg!(windows) {
        format!("{}.exe", name)
    } else {
        name.to_string()
    };
    let primary = if release { "release" } else { "debug" };
    let secondary = if release { "debug" } else { "release" };

    [primary, secondary]
        .into_iter()
        .map(|profile| workspace_root.join("target").join(profile).join(&bin_name))
        .find(|path| path.exists())
}

async fn build_workspace_binary(workspace_root: &Path, name: &str, release: bool) -> Result<()> {
    let mut command = tokio::process::Command::new("cargo");

    // Map bin names to package names if necessary
    let package = match name {
        "flux-runtime" => "runtime",
        "flux-server" => "server",
        _ => name,
    };

    command
        .current_dir(workspace_root)
        .args(["build", "-p", package]);

    if name == "flux-runtime" {
        command.args(["--bin", "flux-runtime"]);
    }

    if release {
        command.arg("--release");
    }

    let status = command
        .status()
        .await
        .context(format!("failed to build {}", name))?;
    if !status.success() {
        anyhow::bail!("failed to build {}", name)
    }

    Ok(())
}
