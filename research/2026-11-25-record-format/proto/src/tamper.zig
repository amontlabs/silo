//! Named changes to a whole record, and what verify says of each. Run by `silorec tamper` (which
//! prints the table in the README) and by the tests.
const std = @import("std");
const format = @import("format.zig");
const verify = @import("verify.zig");
const writer = @import("writer.zig");
const harness = @import("harness.zig");

pub const Rec = struct {
    gpa: std.mem.Allocator,
    segs: std.ArrayList([]u8),
    head: verify.Head,

    pub fn load(gpa: std.mem.Allocator, dir: std.fs.Dir) !Rec {
        const hb = try dir.readFileAlloc(gpa, "head", 4096);
        defer gpa.free(hb);
        return .{ .gpa = gpa, .segs = try verify.readSegments(gpa, dir), .head = try verify.parseHead(gpa, hb, "c1") };
    }

    pub fn clone(r: Rec) !Rec {
        var segs: std.ArrayList([]u8) = .empty;
        for (r.segs.items) |s| try segs.append(r.gpa, try r.gpa.dupe(u8, s));
        return .{ .gpa = r.gpa, .segs = segs, .head = r.head };
    }

    pub fn deinit(r: *Rec) void {
        harness.freeSegs(r.gpa, &r.segs);
    }

    pub fn check(r: Rec, head: bool, blobs: ?verify.Blobs) verify.Result {
        var views: [64][]const u8 = undefined;
        for (r.segs.items, 0..) |s, i| views[i] = s;
        return verify.verify(views[0..r.segs.items.len], .{ .computer = "c1", .head = if (head) r.head else null, .blobs = blobs });
    }
};

pub const Frame = struct { off: usize, len: usize };

pub fn frames(gpa: std.mem.Allocator, seg: []const u8) !std.ArrayList(Frame) {
    var out: std.ArrayList(Frame) = .empty;
    var f = format.Frames{ .bytes = seg };
    while (true) switch (f.next()) {
        .end, .bad => break,
        .line => |l| try out.append(gpa, .{ .off = l.off, .len = format.frame_header_size + l.payload.len }),
    };
    return out;
}

fn splice(gpa: std.mem.Allocator, seg: []u8, off: usize, cut: usize, insert: []const u8) ![]u8 {
    const out = try std.mem.concat(gpa, u8, &.{ seg[0..off], insert, seg[off + cut ..] });
    gpa.free(seg);
    return out;
}

fn framed(gpa: std.mem.Allocator, payload: []const u8) ![]u8 {
    return std.mem.concat(gpa, u8, &.{ &format.frameHeader(payload), payload });
}

/// Rewrites every line from frame `fi` of segment `si` onward so the chain holds again, as someone
/// with the user's rights could: new `prev`, new CRCs, new segment headers. Returns the new head.
pub fn rechain(r: *Rec, si: usize, fi: usize) !verify.Head {
    const gpa = r.gpa;
    var prev = format.zero_hash;
    var seq: u64 = 0;
    for (r.segs.items, 0..) |*seg, s| {
        var fs = try frames(gpa, seg.*);
        defer fs.deinit(gpa);
        if (s >= si) {
            var h = try format.Header.decode(seg.*[0..format.header_size]);
            h.prev = prev;
            h.first_seq = seq + 1;
            seg.*[0..format.header_size].* = h.encode();
        }
        var out: std.ArrayList(u8) = .empty;
        try out.appendSlice(gpa, seg.*[0..format.header_size]);
        for (fs.items, 0..) |f, i| {
            const payload = seg.*[f.off + 8 .. f.off + f.len];
            seq += 1;
            if (s > si or (s == si and i >= fi)) {
                const t = std.mem.indexOf(u8, payload, ",\"time\":\"").?;
                var pre: [160]u8 = undefined;
                const p = try std.fmt.bufPrint(&pre, "{{\"v\":1,\"seq\":{d},\"prev\":\"{s}\"", .{ seq, format.hex(prev) });
                const np = try std.mem.concat(gpa, u8, &.{ p, payload[t..] });
                defer gpa.free(np);
                try out.appendSlice(gpa, &format.frameHeader(np));
                try out.appendSlice(gpa, np);
                prev = format.lineHash(np);
            } else {
                try out.appendSlice(gpa, seg.*[f.off .. f.off + f.len]);
                prev = format.lineHash(payload);
            }
        }
        gpa.free(seg.*);
        seg.* = try out.toOwnedSlice(gpa);
    }
    return .{ .seq = seq, .hash = prev };
}

pub const Case = struct {
    name: []const u8,
    head: bool = true,
    run: *const fn (r: *Rec) anyerror!void,
};

fn payloadOf(seg: []const u8, f: Frame) []const u8 {
    return seg[f.off + 8 .. f.off + f.len];
}

/// Changes the line's number in its source's count by one digit: same length, still a valid line
/// of the right shape, so only the chain can tell.
fn editPayload(gpa: std.mem.Allocator, seg: []const u8, f: Frame) ![]u8 {
    const p = try gpa.dupe(u8, payloadOf(seg, f));
    const i = std.mem.indexOf(u8, p, "\"n\":").? + 4;
    p[i] = if (p[i] == '9') '1' else p[i] + 1;
    return p;
}

const cases = [_]Case{
    .{ .name = "One byte of a line changed", .run = struct {
        fn f(r: *Rec) !void {
            r.segs.items[1][200] ^= 0x01;
        }
    }.f },
    .{ .name = "A line changed and its CRC written again", .run = struct {
        fn f(r: *Rec) !void {
            var fs = try frames(r.gpa, r.segs.items[1]);
            defer fs.deinit(r.gpa);
            const p = try editPayload(r.gpa, r.segs.items[1], fs.items[1]);
            defer r.gpa.free(p);
            const nf = try framed(r.gpa, p);
            defer r.gpa.free(nf);
            r.segs.items[1] = try splice(r.gpa, r.segs.items[1], fs.items[1].off, fs.items[1].len, nf);
        }
    }.f },
    .{ .name = "The last line changed and its CRC written again", .run = struct {
        fn f(r: *Rec) !void {
            const s = r.segs.items.len - 1;
            var fs = try frames(r.gpa, r.segs.items[s]);
            defer fs.deinit(r.gpa);
            const last = fs.items[fs.items.len - 1];
            const p = try editPayload(r.gpa, r.segs.items[s], last);
            defer r.gpa.free(p);
            const nf = try framed(r.gpa, p);
            defer r.gpa.free(nf);
            r.segs.items[s] = try splice(r.gpa, r.segs.items[s], last.off, last.len, nf);
        }
    }.f },
    .{ .name = "A line removed", .run = struct {
        fn f(r: *Rec) !void {
            var fs = try frames(r.gpa, r.segs.items[1]);
            defer fs.deinit(r.gpa);
            r.segs.items[1] = try splice(r.gpa, r.segs.items[1], fs.items[2].off, fs.items[2].len, "");
        }
    }.f },
    .{ .name = "Two lines swapped", .run = struct {
        fn f(r: *Rec) !void {
            var fs = try frames(r.gpa, r.segs.items[1]);
            defer fs.deinit(r.gpa);
            const a = fs.items[1];
            const b = fs.items[2];
            const s = r.segs.items[1];
            const swapped = try std.mem.concat(r.gpa, u8, &.{ s[b.off .. b.off + b.len], s[a.off .. a.off + a.len] });
            defer r.gpa.free(swapped);
            r.segs.items[1] = try splice(r.gpa, s, a.off, a.len + b.len, swapped);
        }
    }.f },
    .{ .name = "A forged line inserted, with a valid CRC", .run = struct {
        fn f(r: *Rec) !void {
            var fs = try frames(r.gpa, r.segs.items[1]);
            defer fs.deinit(r.gpa);
            const p = try editPayload(r.gpa, r.segs.items[1], fs.items[1]);
            defer r.gpa.free(p);
            const nf = try framed(r.gpa, p);
            defer r.gpa.free(nf);
            r.segs.items[1] = try splice(r.gpa, r.segs.items[1], fs.items[2].off, 0, nf);
        }
    }.f },
    .{ .name = "The last lines cut at a line's end", .run = cutTail },
    .{ .name = "The last lines cut at a line's end, no head", .head = false, .run = cutTail },
    .{ .name = "The last segment deleted", .run = struct {
        fn f(r: *Rec) !void {
            r.gpa.free(r.segs.pop().?);
        }
    }.f },
    .{ .name = "A segment in the middle deleted", .run = struct {
        fn f(r: *Rec) !void {
            r.gpa.free(r.segs.orderedRemove(1));
        }
    }.f },
    .{ .name = "A header byte changed", .run = struct {
        fn f(r: *Rec) !void {
            r.segs.items[1][33] ^= 0x01;
        }
    }.f },
    .{ .name = "A segment of another computer put in its place", .run = struct {
        fn f(r: *Rec) !void {
            var h = try format.Header.decode(r.segs.items[1][0..format.header_size]);
            h.computer = 2;
            r.segs.items[1][0..format.header_size].* = h.encode();
        }
    }.f },
    .{ .name = "Garbage appended to the last segment", .run = struct {
        fn f(r: *Rec) !void {
            const s = r.segs.items.len - 1;
            const g = try std.mem.concat(r.gpa, u8, &.{ r.segs.items[s], "\x10\x00\x00\x00garbagegarbage!!" });
            r.gpa.free(r.segs.items[s]);
            r.segs.items[s] = g;
        }
    }.f },
    .{ .name = "A line changed and every line after it chained again, old head kept elsewhere", .run = struct {
        fn f(r: *Rec) !void {
            _ = try editAndRechain(r);
        }
    }.f },
    .{ .name = "A line changed and every line after it chained again, head rewritten too", .run = struct {
        fn f(r: *Rec) !void {
            r.head = try editAndRechain(r);
        }
    }.f },
};

fn editAndRechain(r: *Rec) !verify.Head {
    var fs = try frames(r.gpa, r.segs.items[1]);
    defer fs.deinit(r.gpa);
    const p = try editPayload(r.gpa, r.segs.items[1], fs.items[1]);
    defer r.gpa.free(p);
    const nf = try framed(r.gpa, p);
    defer r.gpa.free(nf);
    r.segs.items[1] = try splice(r.gpa, r.segs.items[1], fs.items[1].off, fs.items[1].len, nf);
    return rechain(r, 1, 2);
}

fn cutTail(r: *Rec) !void {
    const s = r.segs.items.len - 1;
    var fs = try frames(r.gpa, r.segs.items[s]);
    defer fs.deinit(r.gpa);
    const keep = fs.items[fs.items.len - 2].off;
    const cut = try r.gpa.dupe(u8, r.segs.items[s][0..keep]);
    r.gpa.free(r.segs.items[s]);
    r.segs.items[s] = cut;
}

pub const Row = struct { name: []const u8, found: bool, what: []const u8 };

/// Writes the workload into `path` and runs every case on a copy of it.
pub fn runAll(gpa: std.mem.Allocator, path: []const u8, rows: *std.ArrayList(Row)) !void {
    std.fs.cwd().deleteTree(path) catch {};
    {
        var w = try writer.Writer.open(gpa, path, "c1", harness.options(2048));
        defer w.close();
        try harness.workload(gpa, &w, null);
    }
    var dir = try std.fs.cwd().openDir(path, .{ .iterate = true });
    defer dir.close();
    var base = try Rec.load(gpa, dir);
    defer base.deinit();
    if (base.segs.items.len < 4) return error.WorkloadTooSmall;
    if (!base.check(true, null).ok()) return error.BaseDoesNotVerify;
    for (cases) |c| {
        var r = try base.clone();
        defer r.deinit();
        try c.run(&r);
        const res = r.check(c.head, null);
        try rows.append(gpa, .{ .name = c.name, .found = !res.ok(), .what = if (res.problem) |p| p.what else "every chain holds" });
    }
    try blobCases(gpa, path, rows);
    try writerCases(gpa, path, rows);
}

fn blobCases(gpa: std.mem.Allocator, path: []const u8, rows: *std.ArrayList(Row)) !void {
    var dir = try std.fs.cwd().openDir(path, .{ .iterate = true });
    defer dir.close();
    var blobs = try dir.openDir("blobs", .{ .iterate = true });
    defer blobs.close();
    // The first blob on disk.
    var walk = try blobs.walk(gpa);
    defer walk.deinit();
    var name: [64]u8 = undefined;
    while (try walk.next()) |e| if (e.kind == .file) {
        name = e.basename[0..64].*;
        break;
    };
    var pb: [80]u8 = undefined;
    const p = try std.fmt.bufPrint(&pb, "{s}/{s}", .{ name[0..2], name });
    const orig = try blobs.readFileAlloc(gpa, p, 1 << 20);
    defer gpa.free(orig);

    try blobs.writeFile(.{ .sub_path = p, .data = "changed" });
    var r1 = try harness.verifyDir(gpa, dir, "c1");
    try rows.append(gpa, .{ .name = "A blob's bytes changed", .found = !r1.ok(), .what = if (r1.problem) |x| x.what else "every chain holds" });

    try blobs.deleteFile(p);
    r1 = try harness.verifyDir(gpa, dir, "c1");
    try rows.append(gpa, .{ .name = "A blob deleted, no fact says so", .found = !r1.ok(), .what = if (r1.problem) |x| x.what else "every chain holds" });

    // The size limit removes it the honest way: a fact names it (`record-size-limit`).
    {
        var w = try writer.Writer.open(gpa, path, "c1", harness.options(2048));
        defer w.close();
        var b = @import("line.zig").Body.init(gpa, .{});
        defer b.deinit();
        try b.author("silo", null);
        try b.str("what", "bulky-removed");
        try b.str("why", "size-limit");
        try b.array("blobs");
        var ref: [71]u8 = undefined;
        @memcpy(ref[0..7], "sha256:");
        ref[7..].* = name;
        try b.itemStr(&ref);
        try b.endArray();
        _ = try w.append(.{ .time_ns = 1795574467000000000, .source = "engine", .count = "h:5b1e44c0", .n = 1000, .is = "fact", .kind = "control", .body = b.buf.items });
        try w.flush();
        try w.writeHead();
    }
    r1 = try harness.verifyDir(gpa, dir, "c1");
    try rows.append(gpa, .{ .name = "A blob deleted by the size limit, a fact names it", .found = !r1.ok(), .what = if (r1.problem) |x| x.what else "every chain holds" });
    try blobs.makePath(name[0..2]);
    try blobs.writeFile(.{ .sub_path = p, .data = orig });
}

fn writerCases(gpa: std.mem.Allocator, path: []const u8, rows: *std.ArrayList(Row)) !void {
    // A byte changed in a line that is not the last of the last segment: the writer must not cut it
    // away as if it were a write it did not finish.
    var dir = try std.fs.cwd().openDir(path, .{ .iterate = true });
    defer dir.close();
    var segs = try verify.readSegments(gpa, dir);
    defer harness.freeSegs(gpa, &segs);
    const last = segs.items.len;
    var nb: [32]u8 = undefined;
    const name = format.segmentName(&nb, @intCast(last));
    var fs = try frames(gpa, segs.items[last - 1]);
    defer fs.deinit(gpa);
    const bytes = try gpa.dupe(u8, segs.items[last - 1]);
    defer gpa.free(bytes);
    bytes[fs.items[0].off + 20] ^= 1;
    try dir.writeFile(.{ .sub_path = name, .data = bytes });
    var refused = false;
    if (writer.Writer.open(gpa, path, "c1", harness.options(2048))) |w0| {
        var w = w0;
        w.close();
    } else |e| refused = e == error.RecordDamaged;
    try rows.append(gpa, .{ .name = "A line changed in the middle of the last segment, then the recorder starts", .found = refused, .what = if (refused) "the recorder refuses to start: record_damaged" else "the recorder cut the record" });
    try dir.writeFile(.{ .sub_path = name, .data = segs.items[last - 1] });
}

test "every tamper case is found except where the format says it cannot be" {
    const gpa = std.testing.allocator;
    var rows: std.ArrayList(Row) = .empty;
    defer rows.deinit(gpa);
    try runAll(gpa, "zig-cache-tamper-test", &rows);
    defer std.fs.cwd().deleteTree("zig-cache-tamper-test") catch {};
    for (rows.items) |r| {
        const expect_found = !(std.mem.endsWith(u8, r.name, "no head") or std.mem.endsWith(u8, r.name, "head rewritten too") or std.mem.endsWith(u8, r.name, "a fact names it"));
        if (r.found != expect_found) {
            std.debug.print("case {s}: found={} ({s})\n", .{ r.name, r.found, r.what });
            return error.TestUnexpectedResult;
        }
    }
}
