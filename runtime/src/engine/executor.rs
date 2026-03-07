use deno_core::{JsRuntime, RuntimeOptions};
use std::collections::HashMap;
use tokio::time::{timeout, Duration};

pub async fn execute_function(
    code: String,
    secrets: HashMap<String, String>,
    payload: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    
    std::thread::spawn(move || {
        let tokio_rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
            
        let result = tokio_rt.block_on(async move {
            let mut rt = JsRuntime::new(RuntimeOptions {
                ..Default::default()
            });

            let secrets_json = serde_json::to_string(&secrets).map_err(|e| e.to_string())?;
            let payload_json = serde_json::to_string(&payload).map_err(|e| e.to_string())?;

            let transformed_code = code.replace("export default", "const __user_fn =");

            let wrapper = format!(r#"
                (async () => {{
                    const ctx = {{
                        env: {},
                        payload: {}
                    }};
                    
                    {}
                    
                    if (typeof __user_fn !== 'function') {{
                        throw new Error("Function must export a default async function or be a function");
                    }}
                    
                    return await __user_fn(ctx);
                }})()
            "#, secrets_json, payload_json, transformed_code);
            
            let res = timeout(Duration::from_secs(5), async {
                let res = rt.execute_script("<anon>", wrapper)
                    .map_err(|e| format!("Execution error: {}", e))?;
                
                let resolved = rt.resolve_value(res).await
                    .map_err(|e| format!("Promise resolution error: {}", e))?;
                    
                let mut scope = rt.handle_scope();
                let local = deno_core::v8::Local::new(&mut scope, resolved);
                
                let json_val = deno_core::serde_v8::from_v8::<serde_json::Value>(&mut scope, local)
                    .map_err(|e| format!("Serialization error: {}", e))?;
                    
                Ok(json_val)
            }).await;
            
            match res {
                Ok(Ok(val)) => Ok(val),
                Ok(Err(e)) => Err(e),
                Err(_) => Err("Function execution timed out after 5 seconds".to_string()),
            }
        });
        
        let _ = tx.send(result);
    });

    match timeout(Duration::from_secs(6), rx).await {
        Ok(Ok(val)) => val,
        Ok(Err(_)) => Err("Thread execution channel dropped".to_string()),
        Err(_) => Err("Thread execution timed out".to_string()),
    }
}
