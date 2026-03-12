//! `flux project` — removed in the framework CLI.
//!
//! Projects are implicit: the current directory with a flux.toml is your project.
//! There is no remote project registry to select or switch between.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum ProjectCommands {
    /// Create a new project
    Create {
        /// Name of the new project
        name: String,
        #[arg(long, default_value = "true")]
        provision_db: bool,
    },
    /// List available projects
    List,
    /// Switch to a specific project
    Use {
        id: String,
    },
}

pub async fn execute(_command: ProjectCommands) -> anyhow::Result<()> {
    anyhow::bail!(
        "`flux project` has been removed.\n  \
         The Flux framework uses the current directory as your project (see flux.toml).\n  \
         Run `flux init` to initialise a new project here."
    )
}
