//! `flux whoami` — removed in the framework CLI.
//!
//! The self-hosted framework has no user accounts or cloud credentials.
//! Project context is determined by the `flux.toml` in the current directory.

pub async fn execute() -> anyhow::Result<()> {
    anyhow::bail!(
        "`flux whoami` has been removed.\n  \
         The Flux framework is self-hosted — there are no user accounts.\n  \
         Project context comes from flux.toml in the current directory."
    )
}
