use crate::client::ApiClient;

pub async fn execute(name: &str) -> anyhow::Result<()> {
    let client = ApiClient::new().await?;

    let url = format!("{}/functions/{}/logs", client.base_url, name);
    println!("Fetching logs for function '{}'...", name);

    let res = client.client
        .get(&url)
        .send()
        .await?;

    res.error_for_status_ref()?;

    // In a real system, this would stream Server-Sent Events or WebSockets.
    // For now, we simulate pulling the latest standard output logs block.
    let logs: Vec<String> = res.json().await.unwrap_or_default();
    
    if logs.is_empty() {
        println!("No logs found.");
    } else {
        for log in logs {
            println!("{}", log);
        }
    }

    Ok(())
}
