use anyhow::Result;
use clap::Args;

use crate::config::resolve_auth;
use crate::grpc::list_logs;

#[derive(Debug, Args)]
pub struct LogsArgs {
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    #[arg(long, env = "FLUX_SERVICE_TOKEN", value_name = "TOKEN")]
    pub token: Option<String>,
    #[arg(long, default_value_t = 50)]
    pub limit: u32,
}

pub async fn execute(args: LogsArgs) -> Result<()> {
    let auth = resolve_auth(args.url, args.token)?;
    let logs = list_logs(&auth.url, &auth.token, args.limit).await?;

    if logs.is_empty() {
        println!("[]");
        return Ok(());
    }

    for log in logs {
        println!(
            "{}\t{}\t{}\t{}",
            log.timestamp,
            log.request_id,
            log.code_version,
            log.status
        );
    }

    Ok(())
}
