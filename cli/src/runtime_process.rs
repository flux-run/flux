use anyhow::{Context, Result};

pub async fn spawn_runtime(
    binary: std::path::PathBuf,
    prog_args: &[String],
    piped: bool,
) -> Result<tokio::process::Child> {
    let mut cmd = tokio::process::Command::new(binary);
    cmd.args(prog_args);

    if piped {
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());
    }

    let child = cmd
        .spawn()
        .context("failed to spawn flux-runtime")?;

    Ok(child)
}
