use std::path::PathBuf;
 
use anyhow::{Context, Result, bail};
 
pub async fn exec_runtime(
    binary: PathBuf,
    prog_args: &[String],
) -> Result<()> {
    #[cfg(unix)]
    {
        // On Unix, we specifically avoid replacement (exec) to allow
        // the parent process to perform cleanup after the child exits.
        use std::os::unix::process::CommandExt;
    }
 
    let mut cmd = tokio::process::Command::new(binary);
    cmd.args(prog_args);
 
    let status = cmd
        .spawn()
        .context("failed to spawn flux-runtime")?
        .wait()
        .await
        .context("flux-runtime exited unexpectedly")?;
 
    if !status.success() {
        bail!("flux-runtime exited with {}", status);
    }
 
    Ok(())
}
