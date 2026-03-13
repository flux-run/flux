// {name} — Flux function (compiled to WASM via zig build-lib)
// Build: zig build-lib handler.zig -target wasm32-freestanding -dynamic -o {name}.wasm
const std = @import("std");

var output_buf: [1024]u8 = undefined;

export fn {name}_handler(input_ptr: [*]const u8, input_len: u32) u64 {
    _ = input_ptr;
    _ = input_len;

    const resp = "{\"ok\":true}";
    @memcpy(output_buf[0..resp.len], resp);

    const ptr: u64 = @intFromPtr(&output_buf);
    return (ptr << 32) | resp.len;
}
