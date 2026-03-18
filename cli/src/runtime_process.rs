use std::path::{Path, PathBuf};

use anyhow::{Result, bail, Context};

pub fn find_workspace_root() -> Option<PathBuf> {
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

pub fn find_runtime_binary(workspace_root: &Path, release: bool) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "flux-runtime.exe"
    } else {
        "flux-runtime"
    };
    let primary = if release { "release" } else { "debug" };
    let secondary = if release { "debug" } else { "release" };

    [primary, secondary]
        .into_iter()
        .map(|profile| workspace_root.join("target").join(profile).join(name))
        .find(|path| path.exists())
}

#[cfg(unix)]
pub async fn exec_runtime(
    workspace_root: PathBuf,
    binary: Option<PathBuf>,
    release: bool,
    prog_args: &[String],
) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let err = if let Some(bin) = binary {
        std::process::Command::new(bin).args(prog_args).exec()
    } else {
        let mut cmd = std::process::Command::new("cargo");
        cmd.current_dir(&workspace_root)
            .args(["run", "-p", "runtime", "--bin", "flux-runtime"]);
        if release {
            cmd.arg("--release");
        }
        cmd.arg("--").args(prog_args).exec()
    };

    bail!("failed to exec flux-runtime: {}", err)
}

#[cfg(not(unix))]
pub async fn exec_runtime(
    workspace_root: PathBuf,
    binary: Option<PathBuf>,
    release: bool,
    prog_args: &[String],
) -> Result<()> {
    let mut cmd = if let Some(bin) = binary {
        let mut c = tokio::process::Command::new(bin);
        c.args(prog_args);
        c
    } else {
        let mut c = tokio::process::Command::new("cargo");
        c.current_dir(&workspace_root)
            .args(["run", "-p", "runtime", "--bin", "flux-runtime"]);
        if release {
            c.arg("--release");
        }
        c.arg("--").args(prog_args);
        c
    };

    let status = cmd
        .spawn()
        .with_context(|| "failed to spawn flux-runtime")?
        .wait()
        .await
        .with_context(|| "flux-runtime exited unexpectedly")?;

    if !status.success() {
        bail!("flux-runtime exited with {}", status);
    }

    Ok(())
}
