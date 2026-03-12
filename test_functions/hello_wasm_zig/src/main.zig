// hello_wasm_zig — Fluxbase "hello world" function in Zig.
//
// Build:
//   zig build -Dtarget=wasm32-wasip1 -Doptimize=ReleaseSmall
//
// Deploy:
//   flux deploy   (runs the build command from flux.json, then uploads handler.wasm)
//
// Invoke:
//   flux invoke hello_wasm_zig '{"name":"Alice"}'
//   # → {"message":"Hello Alice!"}

const std = @import("std");

// ── Host imports ──────────────────────────────────────────────────────────────

extern "fluxbase" fn log(level: i32, ptr: [*]const u8, len: i32) void;

// ── Allocator ─────────────────────────────────────────────────────────────────

var gpa = std.heap.GeneralPurposeAllocator(.{}){};
const allocator = gpa.allocator();

export fn __flux_alloc(size: i32) i32 {
    const buf = allocator.alloc(u8, @intCast(size)) catch return 0;
    return @intCast(@intFromPtr(buf.ptr));
}

// ── ABI helpers ───────────────────────────────────────────────────────────────

fn logStr(msg: []const u8) void {
    log(1, msg.ptr, @intCast(msg.len));
}

fn writeResult(json: []const u8) i32 {
    const len: u32 = @intCast(json.len);
    const buf = allocator.alloc(u8, 4 + json.len) catch return 0;
    buf[0] = @intCast(len & 0xff);
    buf[1] = @intCast((len >> 8) & 0xff);
    buf[2] = @intCast((len >> 16) & 0xff);
    buf[3] = @intCast((len >> 24) & 0xff);
    @memcpy(buf[4..], json);
    return @intCast(@intFromPtr(buf.ptr));
}

// ── Handler ───────────────────────────────────────────────────────────────────

export fn handle(payload_ptr: i32, payload_len: i32) i32 {
    const payload: []const u8 = @as(
        [*]const u8,
        @ptrFromInt(@as(usize, @intCast(payload_ptr))),
    )[0..@intCast(payload_len)];

    logStr("hello_wasm_zig: executing");

    // Naive "name" field extraction
    var name_value: []const u8 = "world";
    if (std.mem.indexOf(u8, payload, "\"name\"")) |idx| {
        const after = payload[idx + 6 ..];
        if (std.mem.indexOf(u8, after, "\"")) |q1| {
            const rest = after[q1 + 1 ..];
            if (std.mem.indexOf(u8, rest, "\"")) |q2| {
                name_value = rest[0..q2];
            }
        }
    }

    const json = std.fmt.allocPrint(
        allocator,
        "{{\"output\":{{\"message\":\"Hello {s}!\"}}}}",
        .{name_value},
    ) catch return 0;

    return writeResult(json);
}
