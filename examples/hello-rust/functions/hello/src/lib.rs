use serde::{Deserialize, Serialize};

// Input and output types are validated against flux.json "schema" at the gateway.
#[derive(Deserialize)]
pub struct Input {
    // TODO: add your input fields
}

#[derive(Serialize)]
pub struct Output {
    ok: bool,
}

#[no_mangle]
pub extern "C" fn hello_handler(ptr: i32, len: i32) -> i64 {
    // Flux calls this function with JSON-encoded input.
    let input_bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
    let _input: Input = serde_json::from_slice(input_bytes).unwrap();

    let output = Output { ok: true };
    let out_bytes = serde_json::to_vec(&output).unwrap();

    // Return pointer+length packed into an i64.
    let out_ptr = out_bytes.as_ptr() as i64;
    let out_len = out_bytes.len() as i64;
    std::mem::forget(out_bytes);
    (out_ptr << 32) | out_len
}
