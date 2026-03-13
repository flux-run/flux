// Build hello → WASM
// Install Zig >= 0.12: https://ziglang.org/download/
const std = @import("std");

pub fn build(b: *std.Build) void {
    const lib = b.addSharedLibrary(.{
        .name = "hello",
        .root_source_file = b.path("handler.zig"),
        .target = b.resolveTargetQuery(.{
            .cpu_arch = .wasm32,
            .os_tag   = .wasi,
        }),
        .optimize = .ReleaseSmall,
    });
    lib.rdynamic = true;
    b.installArtifact(lib);
}
