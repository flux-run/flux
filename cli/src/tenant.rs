//! `flux tenant` — removed in the framework CLI.
//!
//! The Flux framework is single-tenant by design; multi-tenancy is handled
//! at the application layer, not the CLI layer.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum TenantCommands {
    /// Create a new tenant (organization)
    Create {
        /// Name of the new organization
        name: String,
    },
    /// List available tenants
    List,
    /// Switch to a specific tenant
    Use {
        id: String,
    },
}

pub async fn execute(_command: TenantCommands) -> anyhow::Result<()> {
    anyhow::bail!(
        "`flux tenant` has been removed.\n  \
         The Flux framework is single-tenant — multi-tenancy lives in your app code.\n  \
         Configure local services in flux.toml."
    )
}
