const std = @import("std");

// A recorder for the boundary's lines: #386's writer, unchanged, behind #370's sources' protocol.
// #386's files are copied next to this folder's at build time.
pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{ .preferred_optimize_mode = .ReleaseSafe });
    const wf = b.addWriteFiles();
    const from386 = "../../2026-11-25-record-format/proto/src/";
    inline for (.{ "format.zig", "crash.zig", "redact.zig", "line.zig", "writer.zig", "verify.zig" }) |f| _ = wf.addCopyFile(b.path(from386 ++ f), f);
    _ = wf.addCopyFile(b.path("src/recorder.zig"), "recorder.zig");
    const mod = b.createModule(.{ .root_source_file = wf.getDirectory().path(b, "recorder.zig"), .target = target, .optimize = optimize, .link_libc = true });
    b.installArtifact(b.addExecutable(.{ .name = "recorder", .root_module = mod }));
}
