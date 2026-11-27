const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const mod = b.createModule(.{ .root_source_file = b.path("src/main.zig"), .target = target, .optimize = optimize, .link_libc = true });
    mod.linkSystemLibrary("ssl", .{});
    mod.linkSystemLibrary("crypto", .{});
    mod.linkSystemLibrary("z", .{});
    mod.linkSystemLibrary("zstd", .{});
    const exe = b.addExecutable(.{ .name = "boundary", .root_module = mod });
    b.installArtifact(exe);

    const test_step = b.step("test", "Run the unit tests");
    inline for (.{ "src/stream.zig", "src/usage.zig", "src/prices.zig", "src/budget.zig", "src/journal.zig", "src/spend.zig", "src/outside.zig" }) |f| {
        const tm = b.createModule(.{ .root_source_file = b.path(f), .target = target, .optimize = optimize, .link_libc = true });
        tm.linkSystemLibrary("z", .{});
        const t = b.addTest(.{ .root_module = tm });
        test_step.dependOn(&b.addRunArtifact(t).step);
    }
}
