use anyhow::Result;
use runtime::artifact::build_artifact;
use runtime::boot_runtime_artifact;
use runtime::isolate_pool::ExecutionContext;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn boot_execution_captures_logs_and_listener_registration() -> Result<()> {
    let artifact = build_artifact(
        "server.js",
        r#"
console.log("boot start");
Deno.serve(() => new Response("ok"));
"#,
    );

    let boot = boot_runtime_artifact(&artifact, ExecutionContext::new(artifact.code_version().to_string()))
        .await?;

    assert!(boot.is_server_mode, "boot should detect listener registration");
    assert_eq!(boot.result.status, "ok");
    assert_eq!(boot.result.error, None);
    assert_eq!(boot.result.body["phase"], "boot");
    assert_eq!(boot.result.body["listener_mode"], true);
    assert_eq!(boot.result.logs.len(), 1);
    assert!(boot.result.logs[0].message.contains("boot start"));

    Ok(())
}