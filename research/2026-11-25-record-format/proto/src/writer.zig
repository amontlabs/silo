//! The recorder's writer for one chain: crash-safe append, the hash chain, blobs stored once, the
//! head, redaction. It is the only writer of its folder (`processes`: only the recorder writes the
//! record), so it needs no lock and never counts again behind another writer.
const std = @import("std");
const format = @import("format.zig");
const crash = @import("crash.zig");
const Redactor = @import("redact.zig").Redactor;
const Body = @import("line.zig").Body;
const linez = @import("line.zig");

pub const Options = struct {
    /// #370 closes a segment at 64 MiB or at local midnight.
    max_segment: u64 = 64 * 1024 * 1024,
    /// The head is rewritten after this many lines; the recorder does it every hour and when it
    /// stops. Lines are the prototype's stand-in for the hour.
    head_every: u64 = 10_000,
    redactor: Redactor = .{},
};

pub const Line = struct {
    time_ns: i128,
    source: []const u8,
    /// The id of the source's count (`sources-number-their-lines`) and the line's number in it.
    count: []const u8,
    n: u64,
    is: []const u8,
    kind: []const u8,
    /// The kind's keys, built with `line.Body`.
    body: []const u8,
};

pub const Appended = enum { written, duplicate };

const SourcePos = struct { count: []u8, n: u64 };

pub const Writer = struct {
    gpa: std.mem.Allocator,
    dir: std.fs.Dir,
    blobs: std.fs.Dir,
    computer: []u8,
    computer_id: u64,
    opts: Options,
    seg: std.fs.File,
    seg_number: u32,
    /// Bytes of the segment on disk, and of the batch not yet written.
    seg_size: u64,
    pending: std.ArrayList(u8) = .empty,
    next_seq: u64,
    last_hash: [32]u8,
    flushed_seq: u64,
    flushed_hash: [32]u8,
    lines_since_head: u64 = 0,
    sources: std.StringHashMapUnmanaged(SourcePos) = .empty,
    /// What the start found and did, for the line it writes about it.
    cut: ?struct { segment: u32, offset: u64, bytes: u64 } = null,
    deferred_blobs: std.ArrayList([]u8) = .empty,

    pub const OpenError = error{RecordDamaged} || anyerror;

    /// Opens a chain, after a stop of any kind. It removes what a write left half done and cuts an
    /// unfinished last line, which its source sends again since it was never acknowledged.
    pub fn open(gpa: std.mem.Allocator, path: []const u8, computer: []const u8, opts: Options) OpenError!Writer {
        try std.fs.cwd().makePath(path);
        var dir = try std.fs.cwd().openDir(path, .{ .iterate = true });
        errdefer dir.close();
        try dir.makePath("blobs");
        var blobs = try dir.openDir("blobs", .{ .iterate = true });
        errdefer blobs.close();
        try removeTmp(gpa, dir);
        try removeTmpBlobs(gpa, blobs);

        var w = Writer{
            .gpa = gpa,
            .dir = dir,
            .blobs = blobs,
            .computer = try gpa.dupe(u8, computer),
            .computer_id = format.computerId(computer) orelse return error.NotAComputer,
            .opts = opts,
            .seg = undefined,
            .seg_number = 0,
            .seg_size = 0,
            .next_seq = 1,
            .last_hash = format.zero_hash,
            .flushed_seq = 0,
            .flushed_hash = format.zero_hash,
        };
        errdefer w.freeMemory();

        var segs = @import("verify.zig").readSegments(gpa, dir) catch return error.RecordDamaged;
        defer {
            for (segs.items) |s| gpa.free(s);
            segs.deinit(gpa);
        }
        if (segs.items.len == 0) {
            try w.newSegment(1);
        } else {
            for (segs.items, 0..) |bytes, i| try w.scan(bytes, @intCast(i + 1), i == segs.items.len - 1);
            var nb: [32]u8 = undefined;
            w.seg_number = @intCast(segs.items.len);
            w.seg = try dir.openFile(format.segmentName(&nb, w.seg_number), .{ .mode = .read_write });
            if (w.cut) |c| {
                // Keep the cut bytes beside the segment, then cut: nothing leaves without a trace.
                const last = segs.items[segs.items.len - 1];
                var cb: [48]u8 = undefined;
                const cname = std.fmt.bufPrint(&cb, "{d:0>6}.cut-{d}", .{ c.segment, c.offset }) catch unreachable;
                try dir.writeFile(.{ .sub_path = cname, .data = last[c.offset..] });
                try w.seg.setEndPos(c.offset);
                try w.seg.sync();
                try std.posix.fsync(dir.fd);
            }
            w.seg_size = try w.seg.getEndPos();
        }
        w.flushed_seq = w.next_seq - 1;
        w.flushed_hash = w.last_hash;
        if (w.cut) |c| {
            var b = Body.init(gpa, .{});
            defer b.deinit();
            try b.author("silo", null);
            try b.str("what", "record-tail-cut");
            try b.int("segment", c.segment);
            try b.int("offset", @intCast(c.offset));
            try b.int("bytes", @intCast(c.bytes));
            try w.appendOwn("fact", "control", b.buf.items);
            try w.flush();
        }
        return w;
    }

    pub fn close(w: *Writer) void {
        w.seg.close();
        w.blobs.close();
        w.dir.close();
        w.freeMemory();
    }

    fn freeMemory(w: *Writer) void {
        var it = w.sources.iterator();
        while (it.next()) |e| {
            w.gpa.free(e.key_ptr.*);
            w.gpa.free(e.value_ptr.count);
        }
        w.sources.deinit(w.gpa);
        w.pending.deinit(w.gpa);
        for (w.deferred_blobs.items) |b| w.gpa.free(b);
        w.deferred_blobs.deinit(w.gpa);
        w.gpa.free(w.computer);
    }

    /// Adds a line to the batch. It is on disk, and may be acknowledged, once `flush` returns.
    pub fn append(w: *Writer, l: Line) !Appended {
        if (w.sources.getPtr(l.source)) |pos| {
            if (std.mem.eql(u8, pos.count, l.count)) {
                if (l.n <= pos.n and crash.mutant != .no_dedupe) return .duplicate;
                if (l.n > pos.n + 1) try w.gapLine(l.source, l.count, pos.n + 1, l.n - 1);
            }
        }
        var src: std.ArrayList(u8) = .empty;
        defer src.deinit(w.gpa);
        {
            var b = Body.init(w.gpa, w.opts.redactor);
            defer b.deinit();
            try b.str("id", l.source);
            try b.str("count", l.count);
            try b.int("n", @intCast(l.n));
            try src.appendSlice(w.gpa, b.buf.items);
        }
        try w.appendLine(l.time_ns, src.items, l.is, l.kind, l.body);
        try w.notePos(l.source, l.count, l.n);
        return .written;
    }

    /// A line the recorder writes itself: a gap, a cut. Its source is the recorder, with no count:
    /// nothing stands between it and the disk to lose a line.
    fn appendOwn(w: *Writer, is: []const u8, kind: []const u8, body: []const u8) !void {
        try w.appendLine(std.time.nanoTimestamp(), "\"id\":\"recorder\"", is, kind, body);
    }

    fn gapLine(w: *Writer, source: []const u8, count: []const u8, from: u64, to: u64) !void {
        var b = Body.init(w.gpa, .{});
        defer b.deinit();
        try b.str("of", source);
        try b.str("count", count);
        try b.array("missing");
        try b.itemInt(@intCast(from));
        try b.itemInt(@intCast(to));
        try b.endArray();
        try w.appendOwn("fact", "gap", b.buf.items);
    }

    fn appendLine(w: *Writer, time_ns: i128, source_members: []const u8, is: []const u8, kind: []const u8, body: []const u8) !void {
        if (!linez.isKind(kind)) return error.UnknownKind;
        var tb: [30]u8 = undefined;
        var l: std.ArrayList(u8) = .empty;
        defer l.deinit(w.gpa);
        var head: [256]u8 = undefined;
        try l.appendSlice(w.gpa, std.fmt.bufPrint(&head, "{{\"v\":1,\"seq\":{d},\"prev\":\"{s}\",\"time\":\"{s}\",\"computer\":\"", .{
            w.next_seq, format.hex(w.last_hash), format.formatTime(&tb, time_ns),
        }) catch unreachable);
        try l.appendSlice(w.gpa, w.computer);
        try l.appendSlice(w.gpa, "\",\"source\":{");
        try l.appendSlice(w.gpa, source_members);
        try l.appendSlice(w.gpa, "},\"is\":\"");
        try l.appendSlice(w.gpa, is);
        try l.appendSlice(w.gpa, "\",\"kind\":\"");
        try l.appendSlice(w.gpa, kind);
        try l.append(w.gpa, '"');
        if (body.len > 0) {
            try l.append(w.gpa, ',');
            try l.appendSlice(w.gpa, body);
        }
        try l.append(w.gpa, '}');
        if (l.items.len > format.max_line) return error.LineTooLong;

        const frame_len = format.frame_header_size + l.items.len;
        if (w.seg_size + w.pending.items.len > format.header_size and
            w.seg_size + w.pending.items.len + frame_len > w.opts.max_segment)
        {
            try w.flush();
            try w.newSegment(w.seg_number + 1);
        }
        try w.pending.appendSlice(w.gpa, &format.frameHeader(l.items));
        try w.pending.appendSlice(w.gpa, l.items);
        w.last_hash = format.lineHash(l.items);
        w.next_seq += 1;
    }

    /// Writes the batch with one write and one flush. A failed write or flush stops the recorder:
    /// after it nobody knows what reached the disk (#352 file 02, hole 2).
    pub fn flush(w: *Writer) !void {
        if (w.pending.items.len == 0) return;
        crash.point();
        try crash.write(w.seg, w.pending.items, w.seg_size);
        crash.point();
        try std.posix.fdatasync(w.seg.handle);
        crash.point();
        w.seg_size += w.pending.items.len;
        w.pending.clearRetainingCapacity();
        w.lines_since_head += w.next_seq - 1 - w.flushed_seq;
        w.flushed_seq = w.next_seq - 1;
        w.flushed_hash = w.last_hash;
        for (w.deferred_blobs.items) |b| {
            _ = try w.writeBlob(b);
            w.gpa.free(b);
        }
        w.deferred_blobs.clearRetainingCapacity();
        if (w.lines_since_head >= w.opts.head_every) try w.writeHead();
    }

    fn newSegment(w: *Writer, n: u32) !void {
        if (w.seg_number != 0) w.seg.close();
        const h = format.Header{ .computer = w.computer_id, .segment = n, .first_seq = w.next_seq, .prev = w.last_hash };
        var nb: [32]u8 = undefined;
        const name = format.segmentName(&nb, n);
        var tb: [40]u8 = undefined;
        const tmp = std.fmt.bufPrint(&tb, "{s}.tmp", .{name}) catch unreachable;
        crash.point();
        const f = try w.dir.createFile(tmp, .{ .read = true, .exclusive = true, .mode = 0o600 });
        try crash.write(f, &h.encode(), 0);
        try f.sync();
        crash.point();
        try w.dir.rename(tmp, name);
        try std.posix.fsync(w.dir.fd);
        crash.point();
        w.seg = f;
        w.seg_number = n;
        w.seg_size = format.header_size;
    }

    /// Stores bulky content once, by the hash of its redacted bytes (`bulky-content-once`), and
    /// returns `sha256:<hex>` for a line to point to. It is on disk before any line points to it.
    pub fn putBlob(w: *Writer, content: []const u8) ![71]u8 {
        var clean: std.ArrayList(u8) = .empty;
        defer clean.deinit(w.gpa);
        _ = try w.opts.redactor.apply(w.gpa, &clean, content);
        if (crash.mutant == .line_before_blob) {
            const ref = blobRef(clean.items);
            try w.deferred_blobs.append(w.gpa, try clean.toOwnedSlice(w.gpa));
            return ref;
        }
        return w.writeBlob(clean.items);
    }

    fn blobRef(bytes: []const u8) [71]u8 {
        var r: [71]u8 = undefined;
        @memcpy(r[0..7], "sha256:");
        r[7..71].* = format.hex(format.lineHash(bytes));
        return r;
    }

    fn writeBlob(w: *Writer, bytes: []const u8) ![71]u8 {
        const ref = blobRef(bytes);
        const hx = ref[7..71];
        var pb: [80]u8 = undefined;
        const p = std.fmt.bufPrint(&pb, "{s}/{s}", .{ hx[0..2], hx }) catch unreachable;
        if (w.blobs.access(p, .{})) |_| return ref else |_| {}
        try w.blobs.makePath(hx[0..2]);
        // A folder opened without `iterate` is O_PATH on Linux, which fsync refuses.
        var sub = try w.blobs.openDir(hx[0..2], .{ .iterate = true });
        defer sub.close();
        var tb: [80]u8 = undefined;
        const tmp = std.fmt.bufPrint(&tb, "{s}.tmp", .{hx}) catch unreachable;
        crash.point();
        const f = try sub.createFile(tmp, .{ .mode = 0o600 });
        defer f.close();
        try crash.write(f, bytes, 0);
        try f.sync();
        crash.point();
        try sub.rename(tmp, hx);
        try std.posix.fsync(sub.fd);
        try std.posix.fsync(w.blobs.fd);
        crash.point();
        return ref;
    }

    /// `head`: the hash of the last line on disk, written to a new file and renamed into place.
    pub fn writeHead(w: *Writer) !void {
        var tb: [30]u8 = undefined;
        var buf: [256]u8 = undefined;
        const s = std.fmt.bufPrint(&buf, "{{\"v\":1,\"computer\":\"{s}\",\"seq\":{d},\"hash\":\"{s}\",\"time\":\"{s}\"}}\n", .{
            w.computer, w.flushed_seq, format.hex(w.flushed_hash), format.formatTime(&tb, std.time.nanoTimestamp()),
        }) catch unreachable;
        if (crash.mutant == .head_in_place) {
            crash.point();
            const f = try w.dir.createFile("head", .{ .mode = 0o600 });
            defer f.close();
            try crash.write(f, s, 0);
            try f.sync();
        } else {
            crash.point();
            const f = try w.dir.createFile("head.tmp", .{ .mode = 0o600 });
            defer f.close();
            try crash.write(f, s, 0);
            try f.sync();
            crash.point();
            try w.dir.rename("head.tmp", "head");
            try std.posix.fsync(w.dir.fd);
        }
        crash.point();
        w.lines_since_head = 0;
    }

    fn notePos(w: *Writer, source: []const u8, count: []const u8, n: u64) !void {
        if (w.sources.getPtr(source)) |pos| {
            if (!std.mem.eql(u8, pos.count, count)) {
                w.gpa.free(pos.count);
                pos.count = try w.gpa.dupe(u8, count);
            }
            pos.n = @max(pos.n, n);
            return;
        }
        try w.sources.put(w.gpa, try w.gpa.dupe(u8, source), .{ .count = try w.gpa.dupe(u8, count), .n = n });
    }

    /// Reads one segment at start: the chain's last line, and each source's last number, which the
    /// recorder needs to write a line sent again only once. The recorder keeps these in the
    /// segment's index; the prototype reads them from the lines.
    fn scan(w: *Writer, bytes: []const u8, n: u32, last: bool) !void {
        if (bytes.len < format.header_size) {
            // A segment whose header was never whole is only possible as `.tmp`, removed above.
            return error.RecordDamaged;
        }
        const h = format.Header.decode(bytes[0..format.header_size]) catch return error.RecordDamaged;
        if (h.segment != n or h.first_seq != w.next_seq or !std.mem.eql(u8, &h.prev, &w.last_hash)) return error.RecordDamaged;
        var frames = format.Frames{ .bytes = bytes };
        while (true) switch (frames.next()) {
            .end => break,
            .bad => |b| {
                if (last and (b.torn or crash.mutant == .cut_any_bad_frame)) {
                    w.cut = .{ .segment = n, .offset = b.off, .bytes = bytes.len - b.off };
                    break;
                }
                return error.RecordDamaged;
            },
            .line => |l| {
                var arena = std.heap.ArenaAllocator.init(w.gpa);
                defer arena.deinit();
                const v = std.json.parseFromSliceLeaky(std.json.Value, arena.allocator(), l.payload, .{}) catch return error.RecordDamaged;
                if (v != .object) return error.RecordDamaged;
                const seq = v.object.get("seq") orelse return error.RecordDamaged;
                if (seq != .integer or seq.integer != w.next_seq) return error.RecordDamaged;
                if (v.object.get("source")) |s| if (s == .object) {
                    const id = s.object.get("id");
                    const count = s.object.get("count");
                    const num = s.object.get("n");
                    if (id != null and count != null and num != null and id.? == .string and count.? == .string and num.? == .integer)
                        try w.notePos(id.?.string, count.?.string, @intCast(num.?.integer));
                };
                w.last_hash = format.lineHash(l.payload);
                w.next_seq += 1;
            },
        };
    }
};

fn removeTmp(gpa: std.mem.Allocator, dir: std.fs.Dir) !void {
    var names: std.ArrayList([]u8) = .empty;
    defer {
        for (names.items) |x| gpa.free(x);
        names.deinit(gpa);
    }
    var it = dir.iterate();
    while (try it.next()) |e| if (e.kind == .file and std.mem.endsWith(u8, e.name, ".tmp")) try names.append(gpa, try gpa.dupe(u8, e.name));
    for (names.items) |x| try dir.deleteFile(x);
    if (names.items.len > 0) try std.posix.fsync(dir.fd);
}

fn removeTmpBlobs(gpa: std.mem.Allocator, blobs: std.fs.Dir) !void {
    var subs: std.ArrayList([]u8) = .empty;
    defer {
        for (subs.items) |x| gpa.free(x);
        subs.deinit(gpa);
    }
    var it = blobs.iterate();
    while (try it.next()) |e| if (e.kind == .directory) try subs.append(gpa, try gpa.dupe(u8, e.name));
    for (subs.items) |s| {
        var d = try blobs.openDir(s, .{ .iterate = true });
        defer d.close();
        try removeTmp(gpa, d);
    }
}
