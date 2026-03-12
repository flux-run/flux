//! fluxbase — Fluxbase WASM SDK for Zig
//!
//! Import this module in your handler:
//!
//! ```zig
//! const fluxbase = @import("fluxbase");
//!
//! pub const handle = fluxbase.makeHandle(myHandler);
//! pub const __flux_alloc = fluxbase.__flux_alloc;
//!
//! fn myHandler(ctx: *fluxbase.Ctx, payload: []const u8, alloc: std.mem.Allocator) ![]const u8 {
//!     ctx.log("my-function: executing");
//!     return std.fmt.allocPrint(alloc, "{{\"output\":{{\"message\":\"Hello!\"}}}}", .{});
//! }
//! ```
//!
//! ## Build
//! ```
//! zig build -Dtarget=wasm32-wasip1 -Doptimize=ReleaseSmall
//! ```

const std = @import("std");

// ── Host imports ──────────────────────────────────────────────────────────────

extern "fluxbase" fn log(level: i32, ptr: [*]const u8, len: i32) void;
extern "fluxbase" fn secrets_get(
    key_ptr: [*]const u8, key_len: i32,
    out_ptr: [*]u8, out_max: i32,
) i32;
extern "fluxbase" fn http_fetch(
    req_ptr: [*]const u8, req_len: i32,
    out_ptr: [*]u8, out_max: i32,
) i32;

// ── Allocator ─────────────────────────────────────────────────────────────────

var gpa = std.heap.GeneralPurposeAllocator(.{}){};
pub const allocator = gpa.allocator();

/// ABI-required allocator export.
pub export fn __flux_alloc(size: i32) i32 {
    const buf = allocator.alloc(u8, @intCast(size)) catch return 0;
    return @intCast(@intFromPtr(buf.ptr));
}

// ── Ctx ───────────────────────────────────────────────────────────────────────

pub const Ctx = struct {
    /// Emit an info-level log message.
    pub fn logInfo(self: *Ctx, msg: []const u8) void {
        _ = self;
        log(1, msg.ptr, @intCast(msg.len));
    }

    /// Emit a log message at an explicit level (1=info, 2=warn, 3=error).
    pub fn logLevel(self: *Ctx, level: i32, msg: []const u8) void {
        _ = self;
        log(level, msg.ptr, @intCast(msg.len));
    }

    /// Retrieve a secret. Returns null if not found.
    pub fn secret(self: *Ctx, key: []const u8) ?[]const u8 {
        _ = self;
        var buf: [4096]u8 = undefined;
        const n = secrets_get(key.ptr, @intCast(key.len), &buf, buf.len);
        if (n < 0) return null;
        const result = allocator.alloc(u8, @intCast(n)) catch return null;
        @memcpy(result, buf[0..@intCast(n)]);
        return result;
    }

    /// Perform an outbound HTTP request.
    /// `request_json` must match the Fluxbase HTTP request schema.
    /// Returns the response JSON or null on error.
    pub fn fetch(self: *Ctx, request_json: []const u8) ?[]const u8 {
        _ = self;
        const out = allocator.alloc(u8, 65536) catch return null;
        const n = http_fetch(
            request_json.ptr, @intCast(request_json.len),
            out.ptr, @intCast(out.len),
        );
        if (n < 0) return null;
        const result = allocator.alloc(u8, @intCast(n)) catch return null;
        @memcpy(result, out[0..@intCast(n)]);
        return result;
    }
};

// ── Result helpers ────────────────────────────────────────────────────────────

/// Write a result JSON string into `[4-byte LE len][json]` layout and return
/// the pointer to the buffer.  The buffer is heap-allocated and must NOT be
/// freed while the host is reading it.
pub fn writeResult(json: []const u8) i32 {
    const len: u32 = @intCast(json.len);
    const buf = allocator.alloc(u8, 4 + json.len) catch return 0;
    buf[0] = @intCast(len & 0xff);
    buf[1] = @intCast((len >> 8) & 0xff);
    buf[2] = @intCast((len >> 16) & 0xff);
    buf[3] = @intCast((len >> 24) & 0xff);
    @memcpy(buf[4..], json);
    return @intCast(@intFromPtr(buf.ptr));
}

/// Convenience: write `{"output": <outputJson>}`.
pub fn writeOutput(output_json: []const u8) i32 {
    const json = std.fmt.allocPrint(
        allocator,
        "{{\"output\":{s}}}",
        .{output_json},
    ) catch return 0;
    return writeResult(json);
}

/// Convenience: write `{"error": "<message>"}`.
pub fn writeError(message: []const u8) i32 {
    const json = std.fmt.allocPrint(
        allocator,
        "{{\"error\":\"{s}\"}}",
        .{message},
    ) catch return 0;
    return writeResult(json);
}

// ── Handler factory ───────────────────────────────────────────────────────────

/// The signature expected by `makeHandle`.
pub const HandlerFn = fn (ctx: *Ctx, payload: []const u8, alloc: std.mem.Allocator) anyerror![]const u8;

/// Generate a `handle` export function that wraps your handler.
///
/// Usage:
/// ```zig
/// pub const handle = fluxbase.makeHandle(myHandlerFn);
/// ```
pub fn makeHandle(comptime handler: HandlerFn) fn (i32, i32) callconv(.C) i32 {
    return struct {
        pub export fn handle(payload_ptr: i32, payload_len: i32) i32 {
            const payload: []const u8 = @as([*]const u8, @ptrFromInt(@as(usize, @intCast(payload_ptr))))[0..@intCast(payload_len)];
            var ctx = Ctx{};
            const result_json = handler(&ctx, payload, allocator) catch |err| {
                const msg = @errorName(err);
                const json = std.fmt.allocPrint(
                    allocator,
                    "{{\"error\":\"{s}\"}}",
                    .{msg},
                ) catch return 0;
                return writeResult(json);
            };
            const json = std.fmt.allocPrint(
                allocator,
                "{{\"output\":{s}}}",
                .{result_json},
            ) catch return 0;
            return writeResult(json);
        }
    }.handle;
}
