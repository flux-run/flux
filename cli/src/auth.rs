//! `flux login` — removed in the framework CLI.
//!
//! The self-hosted framework does not have a managed cloud service.
//! There is no authentication concept at the CLI level.

pub async fn execute() -> anyhow::Result<()> {
    anyhow::bail!(
        "`flux login` has been removed.\n  \
         The Flux framework is self-hosted and requires no cloud credentials.\n  \
         Run `flux dev` to start the local development stack."
    )
}


