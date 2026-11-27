const std = @import("std");

// The boundary of #374 (itself #347's), with the two ways in, certificates made per host,
// placeholders, the metadata server, name lookups and the recorder link added. #374's modules are
// used as they are: they are copied next to this folder's files at build time, so one copy of each
// is kept in the repository.
pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});
    const wf = b.addWriteFiles();
    const from374 = "../../2026-11-24-spend-budgets/boundary/src/";
    inline for (.{ "usage.zig", "prices.zig", "budget.zig", "journal.zig", "spend.zig", "stream.zig", "ws.zig", "outside.zig" }) |f| _ = wf.addCopyFile(b.path(from374 ++ f), f);
    inline for (.{ "main.zig", "entry.zig", "mint.zig", "swap.zig", "source.zig", "dns.zig", "conf.zig", "placeholder.zig", "h2.zig", "ja4.zig", "keyplan.zig", "hold.zig" }) |f| _ = wf.addCopyFile(b.path("src/" ++ f), f);
    const root = wf.getDirectory();
    const mod = b.createModule(.{ .root_source_file = root.path(b, "main.zig"), .target = target, .optimize = optimize, .link_libc = true });
    inline for (.{ "ssl", "crypto", "z", "zstd", "nghttp2" }) |l| mod.linkSystemLibrary(l, .{});
    const exe = b.addExecutable(.{ .name = "boundary", .root_module = mod });
    b.installArtifact(exe);

    const test_step = b.step("test", "Run the unit tests");
    inline for (.{ "entry.zig", "swap.zig", "dns.zig", "source.zig", "conf.zig", "placeholder.zig", "ja4.zig", "keyplan.zig", "hold.zig" }) |f| {
        const tm = b.createModule(.{ .root_source_file = root.path(b, f), .target = target, .optimize = optimize, .link_libc = true });
        const t = b.addTest(.{ .root_module = tm });
        test_step.dependOn(&b.addRunArtifact(t).step);
    }
}
