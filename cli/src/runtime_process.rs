use std::path::{Path, PathBuf};

use anyhow::{Result, bail, Context};

#[cfg(unix)]
pub async fn exec_runtime(
    binary: PathBuf,
    prog_args: &[String],
) -> Result<()> {
    use std::os::unix::process::CommandExt;

    let err = std::process::Command::new(binary).args(prog_args).exec();
    bail!("failed to exec flux-runtime: {}", err)
}

#[cfg(not(unix))]
pub async fn exec_runtime(
    binary: PathBuf,
    prog_args: &[String],
) -> Result<()> {
    let mut cmd = tokio::process::Command::new(binary);
    cmd.args(prog_args);

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
