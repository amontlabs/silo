//! silorec: a prototype of the recorder's writer and of `silo verify`.
//!
//!   silorec verify <dir> <computer>            the chain, its head and its blobs
//!   silorec print <dir>                        the lines as stored, one per line (`silo record --json`)
//!   silorec tamper <scratch> [mutant]          every named change and what verify says of it
//!   silorec crash <scratch> [mutant]           the systematic crash test
//!   silorec fuzz <scratch> <runs> <seed>       the reader's fuzzer
//!   silorec night <dir> heavy|light full|delta the size of a night
//!   silorec import <dir> <lines.jsonl>         writes lines converted from another record
//!   silorec redact-cost <secrets> <lines>      what redaction costs per line
//!   silorec workload <dir> <max_segment>       the crash test's workload, printing each ack (power-cut test)
//!   silorec start <dir> <max_segment> [rule]   starts the writer on what is there and stops it (power-cut test)
//!   silorec bench <dir> <lines> <per_flush> <bytes> <blob_every>
//!                                              what an acknowledgement costs: <lines> lines of
//!                                              <bytes>, a flush every <per_flush>, a new blob every
//!                                              <blob_every> lines (0: none); prints one JSON line.
//!                                              SILOREC_FLUSH=full|barrier|fsync picks the Mac flush;
//!                                              SILOREC_ACKS=1 prints "<last line> acked" after each
//!                                              flush (the Mac kit's power-cut test, 41)
const std = @import("std");
const format = @import("format.zig");
const verify = @import("verify.zig");
const writer = @import("writer.zig");
const harness = @import("harness.zig");
const tamper = @import("tamper.zig");
const crashtest = @import("crashtest.zig");
const fuzz = @import("fuzz.zig");
const night = @import("night.zig");
const crash = @import("crash.zig");
const redact = @import("redact.zig");
const Body = @import("line.zig").Body;

test {
    _ = format;
    _ = redact;
    _ = @import("line.zig");
    _ = tamper;
    _ = fuzz;
}

fn out(comptime fmt: []const u8, args: anytype) void {
    var buf: [8192]u8 = undefined;
    const s = std.fmt.bufPrint(&buf, fmt, args) catch return;
    std.fs.File.stdout().writeAll(s) catch {};
}

pub fn main() !void {
    var gpa_state = std.heap.GeneralPurposeAllocator(.{}){};
    const gpa = gpa_state.allocator();
    const args = try std.process.argsAlloc(gpa);
    defer std.process.argsFree(gpa, args);
    if (args.len < 2) return usage();
    // SILOREC_START_RULE=after_head runs any command with that start rule (power-cut test).
    if (std.posix.getenv("SILOREC_START_RULE")) |r| writer.start_rule = std.meta.stringToEnum(writer.StartRule, r) orelse return usage();
    if (std.posix.getenv("SILOREC_FLUSH")) |f| writer.flush_mode = std.meta.stringToEnum(writer.Flush, f) orelse return usage();
    const cmd = args[1];
    if (std.mem.eql(u8, cmd, "verify") and args.len == 4) {
        var dir = try std.fs.cwd().openDir(args[2], .{ .iterate = true });
        defer dir.close();
        const r = try harness.verifyDir(gpa, dir, args[3]);
        if (r.problem) |p| {
            out("segment {d}, byte {d}, line {d}: {s}\n", .{ p.segment, p.offset, p.seq, p.what });
            std.process.exit(1);
        }
        out("{d} lines checked in {d} segments, {d} blob references · the chain holds", .{ r.lines, r.segments, r.blobs_checked });
        if (r.after_head > 0) out(" · {d} lines after the head", .{r.after_head});
        out("\n", .{});
    } else if (std.mem.eql(u8, cmd, "print") and args.len == 3) {
        var dir = try std.fs.cwd().openDir(args[2], .{ .iterate = true });
        defer dir.close();
        var segs = try verify.readSegments(gpa, dir);
        defer harness.freeSegs(gpa, &segs);
        const so = std.fs.File.stdout();
        for (segs.items) |s| {
            var f = format.Frames{ .bytes = s };
            while (true) switch (f.next()) {
                .end, .bad => break,
                .line => |l| {
                    try so.writeAll(l.payload);
                    try so.writeAll("\n");
                },
            };
        }
    } else if (std.mem.eql(u8, cmd, "tamper") and args.len >= 3) {
        if (args.len > 3) crash.mutant = std.meta.stringToEnum(crash.Mutant, args[3]) orelse return usage();
        var rows: std.ArrayList(tamper.Row) = .empty;
        defer rows.deinit(gpa);
        try tamper.runAll(gpa, args[2], &rows);
        out("| Change | Found | What verify says |\n|---|---|---|\n", .{});
        for (rows.items) |r| out("| {s} | {s} | {s} |\n", .{ r.name, if (r.found) "yes" else "no", r.what });
    } else if (std.mem.eql(u8, cmd, "crash") and args.len >= 3) {
        const mutant = if (args.len > 3) std.meta.stringToEnum(crash.Mutant, args[3]) orelse return usage() else .none;
        const o = try crashtest.run(gpa, args[2], mutant, 2048);
        out("{{\"mutant\":\"{s}\",\"points\":{d},\"passed\":{d},\"failed\":{d}", .{ @tagName(mutant), o.points, o.passed, o.failed });
        if (o.first_failure) |f| out(",\"first_failure\":{{\"k\":{d},\"what\":\"{s}\"}}", .{ o.first_k, f });
        out("}}\n", .{});
    } else if (std.mem.eql(u8, cmd, "fuzz") and args.len == 5) {
        const path = args[2];
        const runs = try std.fmt.parseInt(u64, args[3], 10);
        const seed = try std.fmt.parseInt(u64, args[4], 10);
        std.fs.cwd().deleteTree(path) catch {};
        {
            var w = try writer.Writer.open(gpa, path, "c1", harness.options(2048));
            defer w.close();
            try harness.workload(gpa, &w, null);
        }
        var dir = try std.fs.cwd().openDir(path, .{ .iterate = true });
        defer dir.close();
        var base = try tamper.Rec.load(gpa, dir);
        defer base.deinit();
        var st = fuzz.Stats{};
        defer st.by_what.deinit(gpa);
        const t0 = std.time.nanoTimestamp();
        try fuzz.run(gpa, base, runs, seed, &st);
        const secs = @as(f64, @floatFromInt(std.time.nanoTimestamp() - t0)) / 1e9;
        out("{{\"runs\":{d},\"seed\":{d},\"unchanged\":{d},\"found\":{d},\"missed\":{d},\"seconds\":{d:.1}}}\n", .{ st.runs, seed, st.same_bytes, st.found, st.missed, secs });
        var it = st.by_what.iterator();
        while (it.next()) |e| out("  {d:>7}  {s}\n", .{ e.value_ptr.*, e.key_ptr.* });
        if (st.missed > 0) std.process.exit(1);
    } else if (std.mem.eql(u8, cmd, "night") and args.len == 5) {
        const load = if (std.mem.eql(u8, args[3], "heavy")) night.heavy else night.light;
        const form = std.meta.stringToEnum(night.SentForm, args[4]) orelse return usage();
        std.fs.cwd().deleteTree(args[2]) catch {};
        var w = try writer.Writer.open(gpa, args[2], "c1", .{});
        const t0 = std.time.nanoTimestamp();
        const st = try night.run(gpa, &w, load, form, 7);
        const secs = @as(f64, @floatFromInt(std.time.nanoTimestamp() - t0)) / 1e9;
        w.close();
        var seg_bytes: u64 = 0;
        var dir = try std.fs.cwd().openDir(args[2], .{ .iterate = true });
        defer dir.close();
        var it = dir.iterate();
        while (try it.next()) |e| if (format.parseSegmentName(e.name) != null) {
            seg_bytes += (try dir.statFile(e.name)).size;
        };
        out("{{\"load\":\"{s}\",\"sent\":\"{s}\",\"lines\":{d},\"segment_bytes\":{d},\"blobs\":{d},\"blob_bytes\":{d},\"raw_resend_bytes\":{d},\"full_lists\":{d},\"write_seconds\":{d:.2},\"by_kind\":{{", .{ load.name, @tagName(form), st.lines, seg_bytes, st.blobs, st.blob_bytes, st.raw_request_bytes, st.full_lists, secs });
        const names = [_][]const u8{ "control", "process", "file", "connection", "model-call", "screen", "meter", "gap", "silence" };
        var first = true;
        for (names, 0..) |n, k| if (st.by_kind[k] > 0) {
            out("{s}\"{s}\":{{\"lines\":{d},\"bytes\":{d}}}", .{ if (first) "" else ",", n, st.by_kind[k], st.bytes_by_kind[k] });
            first = false;
        };
        out("}}}}\n", .{});
    } else if (std.mem.eql(u8, cmd, "workload") and args.len == 4) {
        // The power-cut test traces this run's calls to the disk; an ack is a write to stdout.
        std.fs.cwd().deleteTree(args[2]) catch {};
        var w = try writer.Writer.open(gpa, args[2], "c1", harness.options(try std.fmt.parseInt(u64, args[3], 10)));
        defer w.close();
        try harness.workload(gpa, &w, std.fs.File.stdout().handle);
    } else if (std.mem.eql(u8, cmd, "start") and (args.len == 4 or args.len == 5)) {
        if (args.len == 5) {
            if (std.meta.stringToEnum(writer.StartRule, args[4])) |r| writer.start_rule = r else crash.mutant = std.meta.stringToEnum(crash.Mutant, args[4]) orelse return usage();
        }
        var w = writer.Writer.open(gpa, args[2], "c1", harness.options(try std.fmt.parseInt(u64, args[3], 10))) catch |e| {
            out("the writer refused to start: {s}\n", .{@errorName(e)});
            std.process.exit(3);
        };
        w.close();
        out("started\n", .{});
    } else if (std.mem.eql(u8, cmd, "bench") and args.len == 7) {
        try bench(gpa, args[2], try std.fmt.parseInt(usize, args[3], 10), try std.fmt.parseInt(usize, args[4], 10), try std.fmt.parseInt(usize, args[5], 10), try std.fmt.parseInt(usize, args[6], 10));
    } else if (std.mem.eql(u8, cmd, "import") and args.len == 4) {
        try importLines(gpa, args[2], args[3]);
    } else if (std.mem.eql(u8, cmd, "redact-cost") and args.len == 4) {
        try redactCost(gpa, try std.fmt.parseInt(usize, args[2], 10), args[3]);
    } else return usage();
}

fn usage() void {
    std.debug.print("usage: see the top of src/main.zig\n", .{});
    std.process.exit(2);
}

/// Each input line: {"time_ns":…,"source":…,"count":…,"n":…,"is":…,"kind":…,"body":"<members>"}
fn importLines(gpa: std.mem.Allocator, path: []const u8, input: []const u8) !void {
    std.fs.cwd().deleteTree(path) catch {};
    var w = try writer.Writer.open(gpa, path, "c1", .{});
    defer w.close();
    const bytes = try std.fs.cwd().readFileAlloc(gpa, input, 1 << 30);
    defer gpa.free(bytes);
    var lines = std.mem.tokenizeScalar(u8, bytes, '\n');
    var n: u64 = 0;
    while (lines.next()) |l| {
        const p = try std.json.parseFromSlice(struct { time_ns: i64, source: []const u8, count: []const u8, n: u64, is: []const u8, kind: []const u8, body: []const u8 }, gpa, l, .{});
        defer p.deinit();
        const v = p.value;
        _ = try w.append(.{ .time_ns = v.time_ns, .source = v.source, .count = v.count, .n = v.n, .is = v.is, .kind = v.kind, .body = v.body });
        n += 1;
        if (n % 64 == 0) try w.flush();
    }
    try w.flush();
    try w.writeHead();
    out("{d} lines written\n", .{n});
}

/// What redacting costs: the lines of a file run through a redactor holding `count` secrets.
fn redactCost(gpa: std.mem.Allocator, count: usize, input: []const u8) !void {
    const bytes = try std.fs.cwd().readFileAlloc(gpa, input, 1 << 30);
    defer gpa.free(bytes);
    const secrets = try gpa.alloc(redact.Secret, count);
    defer gpa.free(secrets);
    var vals = try gpa.alloc([40]u8, count);
    defer gpa.free(vals);
    var prng = std.Random.DefaultPrng.init(3);
    for (secrets, 0..) |*s, i| {
        for (&vals[i]) |*c| c.* = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789"[prng.random().uintLessThan(usize, 62)];
        s.* = .{ .name = "s", .value = &vals[i] };
    }
    const r = try redact.Redactor.init(secrets);
    var o: std.ArrayList(u8) = .empty;
    defer o.deinit(gpa);
    var lines: u64 = 0;
    const t0 = std.time.nanoTimestamp();
    var it = std.mem.tokenizeScalar(u8, bytes, '\n');
    while (it.next()) |l| {
        o.clearRetainingCapacity();
        _ = try r.apply(gpa, &o, l);
        lines += 1;
    }
    const ns = std.time.nanoTimestamp() - t0;
    out("{{\"secrets\":{d},\"lines\":{d},\"bytes\":{d},\"ns_per_line\":{d},\"mb_per_s\":{d:.1}}}\n", .{ count, lines, bytes.len, @divFloor(ns, @as(i128, lines)), @as(f64, @floatFromInt(bytes.len)) / (@as(f64, @floatFromInt(ns)) / 1e9) / 1e6 });
}

/// What the writer's acknowledgement costs on this disk, in the flush mode SILOREC_FLUSH picks:
/// the time of each flush (the wait before lines are acknowledged) and of each new blob.
fn bench(gpa: std.mem.Allocator, path: []const u8, lines: usize, per_flush: usize, bytes: usize, blob_every: usize) !void {
    std.fs.cwd().deleteTree(path) catch {};
    var w = try writer.Writer.open(gpa, path, "c1", .{});
    defer w.close();
    const filler = try gpa.alloc(u8, bytes);
    defer gpa.free(filler);
    @memset(filler, 'x');
    var flushes: std.ArrayList(u64) = .empty;
    defer flushes.deinit(gpa);
    var blobs: std.ArrayList(u64) = .empty;
    defer blobs.deinit(gpa);
    const acks = std.posix.getenv("SILOREC_ACKS") != null;
    var timer = try std.time.Timer.start();
    const t0 = timer.read();
    for (0..lines) |i| {
        var b = Body.init(gpa, w.opts.redactor);
        defer b.deinit();
        try b.str("event", "output");
        if (blob_every > 0 and i % blob_every == 0) {
            var cb: [64]u8 = undefined;
            const s0 = timer.read();
            const r = try w.putBlob(std.fmt.bufPrint(&cb, "blob {d} {d}", .{ i, t0 }) catch unreachable);
            try blobs.append(gpa, timer.read() - s0);
            try b.str("blob", &r);
        }
        try b.str("text", filler);
        _ = try w.append(.{ .time_ns = 1795574467000000000 + @as(i128, @intCast(i)) * 1000, .source = "bench", .count = "c", .n = i + 1, .is = "report", .kind = "process", .body = b.buf.items });
        if (i % per_flush == per_flush - 1 or i == lines - 1) {
            const s0 = timer.read();
            try w.flush();
            try flushes.append(gpa, timer.read() - s0);
            if (acks) {
                var nb: [32]u8 = undefined;
                _ = std.posix.write(std.posix.STDOUT_FILENO, std.fmt.bufPrint(&nb, "{d} acked\n", .{i + 1}) catch unreachable) catch {};
            }
        }
    }
    const total = timer.read() - t0;
    const ms = struct {
        fn q(l: []u64, f: f64) f64 {
            if (l.len == 0) return 0;
            std.mem.sort(u64, l, {}, std.sort.asc(u64));
            return @as(f64, @floatFromInt(l[@min(l.len - 1, @as(usize, @intFromFloat(f * @as(f64, @floatFromInt(l.len)))))])) / 1e6;
        }
    }.q;
    out("{{\"flush\":\"{s}\",\"lines\":{d},\"per_flush\":{d},\"bytes\":{d},\"blob_every\":{d},\"flushes\":{d},\"ack_p50_ms\":{d:.3},\"ack_p99_ms\":{d:.3},\"ack_max_ms\":{d:.3},\"blob_p50_ms\":{d:.3},\"blob_p99_ms\":{d:.3},\"lines_per_s\":{d:.0}}}\n", .{
        @tagName(writer.flush_mode), lines,                  per_flush,             bytes,                 blob_every,
        flushes.items.len,           ms(flushes.items, 0.5), ms(flushes.items, 0.99), ms(flushes.items, 1.0), ms(blobs.items, 0.5),
        ms(blobs.items, 0.99),       @as(f64, @floatFromInt(lines)) / (@as(f64, @floatFromInt(total)) / 1e9),
    });
}
