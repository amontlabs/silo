const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{ .preferred_optimize_mode = .ReleaseSafe });
    const mod = b.createModule(.{
        .root_source_file = b.path("src/writelog.zig"),
        .target = target,
        .optimize = optimize,
    });
    const tests = b.addTest(.{ .root_module = mod });
    b.step("test", "Run the unit tests").dependOn(&b.addRunArtifact(tests).step);

    const bench_mod = b.createModule(.{
        .root_source_file = b.path("src/bench.zig"),
        .target = target,
        .optimize = .ReleaseFast,
    });
    bench_mod.addImport("writelog", mod);
    const bench = b.addExecutable(.{ .name = "writelog-bench", .root_module = bench_mod });
    b.installArtifact(bench);
    b.step("bench", "Measure what the hook costs per write").dependOn(&b.addRunArtifact(bench).step);
}
