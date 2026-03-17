use anyhow::Result;
use clap::Args;

use crate::runtime_server::{execute_server_runtime, RuntimeServerOptions};

#[derive(Debug, Args)]
pub struct ServeArgs {
    #[arg(value_name = "ENTRY")]
    pub entry: Option<String>,
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
    #[arg(long)]
    pub skip_verify: bool,
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    #[arg(long, default_value_t = 3000)]
    pub port: u16,
    #[arg(long, default_value_t = 16)]
    pub isolate_pool_size: usize,
    #[arg(long)]
    pub check_only: bool,
    /// Use a release-mode flux-runtime binary if found.
    #[arg(long)]
    pub release: bool,
}

pub async fn execute(args: ServeArgs) -> Result<()> {
    execute_server_runtime(RuntimeServerOptions {
        entry: args.entry,
        url: args.url,
        token: args.token,
        skip_verify: args.skip_verify,
        host: args.host,
        port: args.port,
        isolate_pool_size: args.isolate_pool_size,
        check_only: args.check_only,
        release: args.release,
    })
    .await
}