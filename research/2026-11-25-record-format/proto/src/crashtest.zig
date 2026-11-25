//! The systematic crash test: the workload runs in a child process that dies at its k-th step on
//! the disk, for every k. After each death a new writer starts on what is left, and the test checks:
//!   1. the writer starts (it never refuses what a crash leaves);
//!   2. every line acknowledged before the death is there, and the chain, the head and the blobs
//!      verify;
//!   3. after the sources send again from their first line, each line is there exactly once, in
//!      order, and the chain verifies;
//!   4. no file holds a secret's real value.
const std = @import("std");
const crash = @import("crash.zig");
const writer = @import("writer.zig");
const harness = @import("harness.zig");

pub const Outcome = struct { points: u64 = 0, passed: u64 = 0, failed: u64 = 0, first_failure: ?[]const u8 = null, first_k: u64 = 0 };

pub fn run(gpa: std.mem.Allocator, base: []const u8, mutant: crash.Mutant, max_segment: u64) !Outcome {
    var out = Outcome{};
    try std.fs.cwd().makePath(base);
    var k: u64 = 1;
    while (true) : (k += 1) {
        var pb: [512]u8 = undefined;
        const path = try std.fmt.bufPrint(&pb, "{s}/k{d}", .{ base, k });
        std.fs.cwd().deleteTree(path) catch {};
        const pipe = try std.posix.pipe();
        const pid = try std.posix.fork();
        if (pid == 0) {
            std.posix.close(pipe[0]);
            crash.at = k;
            crash.count = 0;
            crash.mutant = mutant;
            var w = writer.Writer.open(std.heap.page_allocator, path, "c1", harness.options(max_segment)) catch std.posix.exit(3);
            harness.workload(std.heap.page_allocator, &w, pipe[1]) catch std.posix.exit(4);
            std.posix.exit(0);
        }
        std.posix.close(pipe[1]);
        var acked: ?u64 = null;
        var buf: [4096]u8 = undefined;
        var rest: std.ArrayList(u8) = .empty;
        defer rest.deinit(gpa);
        while (true) {
            const n = try std.posix.read(pipe[0], &buf);
            if (n == 0) break;
            try rest.appendSlice(gpa, buf[0..n]);
        }
        std.posix.close(pipe[0]);
        var lines = std.mem.tokenizeScalar(u8, rest.items, '\n');
        while (lines.next()) |l| acked = try std.fmt.parseInt(u64, l, 10);
        const status = std.posix.waitpid(pid, 0).status;
        const code = std.posix.W.EXITSTATUS(status);
        const finished = code == 0;
        if (code != 0 and code != crash.exit_code) return error.ChildFailed;
        out.points += 1;

        crash.at = 0;
        crash.mutant = mutant;
        const failure = check(gpa, path, acked, max_segment) catch |e| @errorName(e);
        crash.mutant = .none;
        if (failure) |f| {
            out.failed += 1;
            if (out.first_failure == null) {
                out.first_failure = f;
                out.first_k = k;
            }
        } else {
            out.passed += 1;
            std.fs.cwd().deleteTree(path) catch {};
        }
        if (finished) break;
    }
    return out;
}

fn check(gpa: std.mem.Allocator, path: []const u8, acked: ?u64, max_segment: u64) !?[]const u8 {
    var w = writer.Writer.open(gpa, path, "c1", harness.options(max_segment)) catch return "the writer refused to start";
    {
        var dir = try std.fs.cwd().openDir(path, .{ .iterate = true });
        defer dir.close();
        const r = try harness.verifyDir(gpa, dir, "c1");
        if (!r.ok()) {
            w.close();
            return r.problem.?.what;
        }
        var seen = try harness.linesOf(gpa, dir);
        defer seen.deinit(gpa);
        if (acked) |a| for (0..a + 1) |i| {
            var found = false;
            for (seen.items) |s| if (s.source == i % 3 and s.n == i / 3 + 1) {
                found = true;
            };
            if (!found) {
                w.close();
                return "a line acknowledged before the crash is missing";
            }
        };
    }
    try harness.workload(gpa, &w, null);
    w.close();
    var dir = try std.fs.cwd().openDir(path, .{ .iterate = true });
    defer dir.close();
    const r = try harness.verifyDir(gpa, dir, "c1");
    if (!r.ok()) return r.problem.?.what;
    var seen = try harness.linesOf(gpa, dir);
    defer seen.deinit(gpa);
    if (seen.items.len != harness.n_lines) return "after sending again, a line is missing or written twice";
    var next = [_]u64{ 1, 1, 1 };
    for (seen.items) |s| {
        if (s.n != next[s.source]) return "after sending again, lines of a source are out of order";
        next[s.source] += 1;
    }
    if (try harness.holdsSecret(gpa, dir)) return "a file holds a secret's real value";
    return null;
}
