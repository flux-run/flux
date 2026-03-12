use flux_wasm_sdk::prelude::*;

#[derive(Deserialize)]
struct Input {
    name: Option<String>,
}

#[derive(Serialize)]
struct Output {
    message: String,
}

// Register `handler` as the entrypoint.  The Flux runtime will call it with
// the request payload deserialised as `Input`.
register_handler!(handler);

fn handler(ctx: &FluxCtx, input: Input) -> FluxResult<Output> {
    let name = input.name.as_deref().unwrap_or("world");

    ctx.log_info(&format!("hello_wasm invoked with name={name}"));

    // Optional: look up a secret (returns None if not configured).
    if let Some(key) = ctx.secrets.get("GREETING_PREFIX") {
        return Ok(Output {
            message: format!("{key} {name}!"),
        });
    }

    Ok(Output {
        message: format!("Hello, {name}!"),
    })
}
