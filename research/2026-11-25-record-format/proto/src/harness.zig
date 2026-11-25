//! The workload the crash test and the tamper test run, and helpers to read a chain back.
const std = @import("std");
const format = @import("format.zig");
const writer = @import("writer.zig");
const verify = @import("verify.zig");
const Body = @import("line.zig").Body;
const redact = @import("redact.zig");

pub const n_lines = 48;
pub const secrets = [_]redact.Secret{
    .{ .name = "stripe-test", .value = "sk_test_51HxQ9aZmB7Kc0vRrT" },
    .{ .name = "github", .value = "ghp_R2d2C3poX9wingL7ukeSkyw4lk3r" },
};

const sources = [_][]const u8{ "engine", "helper:c1", "boundary" };
const counts = [_][]const u8{ "h:5b1e44c0", "k:1", "b:1" };

pub fn options(max_segment: u64) writer.Options {
    return .{ .max_segment = max_segment, .head_every = 1 << 40, .redactor = redact.Redactor.init(&secrets) catch unreachable };
}

/// Sends the 48 lines of the workload in batches. After each flush it writes the index of the last
/// line flushed to `ack`, which is when a source may forget a line.
pub fn workload(gpa: std.mem.Allocator, w: *writer.Writer, ack: ?std.posix.fd_t) !void {
    for (0..n_lines) |i| {
        var b = Body.init(gpa, w.opts.redactor);
        defer b.deinit();
        var kind: []const u8 = "process";
        var is: []const u8 = "report";
        if (i % 5 == 0) {
            kind = "model-call";
            is = "fact";
            var cb: [96]u8 = undefined;
            // Contents repeat every 7 lines, so some blobs are already stored when asked again.
            const m1 = std.fmt.bufPrint(&cb, "tool result {d}: curl -H 'Authorization: Bearer {s}'", .{ i % 7, secrets[0].value }) catch unreachable;
            const r1 = try w.putBlob(m1);
            var cb2: [64]u8 = undefined;
            const r2 = try w.putBlob(std.fmt.bufPrint(&cb2, "reply {d}", .{i}) catch unreachable);
            try b.str("provider", "anthropic");
            try b.array("sent");
            try b.itemStr(&r1);
            try b.endArray();
            try b.str("reply", &r2);
        } else if (i % 7 == 0) {
            kind = "control";
            is = "fact";
            try b.author("you", "p1");
            try b.str("what", "secret-given");
            try b.str("secret", "github");
        } else {
            try b.str("event", "started");
            try b.int("pid", @intCast(4000 + i));
            var cb: [128]u8 = undefined;
            try b.str("command", std.fmt.bufPrint(&cb, "git push https://x:{s}@github.com/a/b", .{secrets[1].value}) catch unreachable);
        }
        _ = try w.append(.{ .time_ns = 1795574467000000000 + @as(i128, @intCast(i)) * 1000, .source = sources[i % 3], .count = counts[i % 3], .n = i / 3 + 1, .is = is, .kind = kind, .body = b.buf.items });
        if (i % 4 == 3 or i == n_lines - 1) {
            try w.flush();
            if (ack) |fd| {
                var nb: [16]u8 = undefined;
                _ = std.posix.write(fd, std.fmt.bufPrint(&nb, "{d}\n", .{i}) catch unreachable) catch {};
            }
        }
        if (i % 9 == 8) try w.writeHead();
    }
    try w.writeHead();
}

pub const Seen = struct { source: usize, n: u64 };

/// The source and number of every line from a source, in the order of the chain.
pub fn linesOf(gpa: std.mem.Allocator, dir: std.fs.Dir) !std.ArrayList(Seen) {
    var segs = try verify.readSegments(gpa, dir);
    defer freeSegs(gpa, &segs);
    var out: std.ArrayList(Seen) = .empty;
    for (segs.items) |s| {
        var fr = format.Frames{ .bytes = s };
        while (true) switch (fr.next()) {
            .end, .bad => break,
            .line => |l| {
                const p = try std.json.parseFromSlice(std.json.Value, gpa, l.payload, .{});
                defer p.deinit();
                const src = p.value.object.get("source").?.object;
                const id = src.get("id").?.string;
                for (sources, 0..) |x, k| if (std.mem.eql(u8, x, id)) {
                    try out.append(gpa, .{ .source = k, .n = @intCast(src.get("n").?.integer) });
                };
            },
        };
    }
    return out;
}

pub fn freeSegs(gpa: std.mem.Allocator, segs: *std.ArrayList([]u8)) void {
    for (segs.items) |s| gpa.free(s);
    segs.deinit(gpa);
}

/// Verifies the chain in `dir`, with its head and its blobs.
pub fn verifyDir(gpa: std.mem.Allocator, dir: std.fs.Dir, computer: []const u8) !verify.Result {
    var segs = try verify.readSegments(gpa, dir);
    defer freeSegs(gpa, &segs);
    var head: ?verify.Head = null;
    if (dir.readFileAlloc(gpa, "head", 4096)) |hb| {
        defer gpa.free(hb);
        head = verify.parseHead(gpa, hb, computer) catch return .{ .problem = .{ .segment = 0, .offset = 0, .seq = 0, .what = "the head cannot be read" } };
    } else |_| {}
    var bd = verify.DirBlobs{ .dir = try dir.openDir("blobs", .{}) };
    defer bd.dir.close();
    const views = try gpa.alloc([]const u8, segs.items.len);
    defer gpa.free(views);
    for (segs.items, views) |s, *v| v.* = s;
    return verify.verify(views, .{ .computer = computer, .head = head, .blobs = bd.blobs() });
}

/// Whether any file under `dir` holds a secret's real value.
pub fn holdsSecret(gpa: std.mem.Allocator, dir: std.fs.Dir) !bool {
    var walk = try dir.walk(gpa);
    defer walk.deinit();
    while (try walk.next()) |e| {
        if (e.kind != .file) continue;
        const bytes = try e.dir.readFileAlloc(gpa, e.basename, 1 << 30);
        defer gpa.free(bytes);
        for (secrets) |s| if (std.mem.indexOf(u8, bytes, s.value) != null) return true;
    }
    return false;
}
