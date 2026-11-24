const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{ .preferred_optimize_mode = .ReleaseSafe });
    const mod = b.createModule(.{
        .root_source_file = b.path("src/main.zig"),
        .target = target,
        .optimize = optimize,
        // clonefile(2) and fcntl come from libSystem on macOS.
        .link_libc = target.result.os.tag.isDarwin(),
    });
    const exe = b.addExecutable(.{ .name = "ckpt", .root_module = mod });
    b.installArtifact(exe);
    const tests = b.addTest(.{ .root_module = mod });
    const run_tests = b.addRunArtifact(tests);
    b.step("test", "Run the unit tests").dependOn(&run_tests.step);
}
