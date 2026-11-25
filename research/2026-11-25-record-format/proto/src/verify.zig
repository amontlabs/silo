//! `silo verify`: checks a chain of the record from its first segment to its head.
//! It reads untrusted bytes, so it never trusts a length or a count it reads, and it uses a fixed
//! amount of memory whatever it is given (`fuzz-untrusted-parsers`).
const std = @import("std");
const format = @import("format.zig");
const line = @import("line.zig");

pub const Chain = enum {
    /// Facts and reports of one computer.
    record,
    /// Opinions of one computer, in a chain of their own (`what-the-record-holds`).
    opinions,
};

pub const Problem = struct {
    segment: u32,
    offset: u64,
    /// The `seq` the line at that place should have.
    seq: u64,
    what: []const u8,
};

pub const Result = struct {
    lines: u64 = 0,
    segments: u32 = 0,
    last_seq: u64 = 0,
    last_hash: [32]u8 = format.zero_hash,
    blobs_checked: u64 = 0,
    blobs_removed: u64 = 0,
    /// Lines after the head's line: nothing outside the record vouches for them yet.
    after_head: u64 = 0,
    problem: ?Problem = null,

    pub fn ok(r: Result) bool {
        return r.problem == null;
    }
};

/// How verify asks whether a blob is on disk and whole. In memory for the fuzzer, a folder for
/// the command.
pub const Blobs = struct {
    ctx: *anyopaque,
    /// .whole, .missing, or .changed (on disk, with other bytes than its name says)
    check: *const fn (ctx: *anyopaque, hex: []const u8) BlobState,
};
pub const BlobState = enum { whole, missing, changed };

pub const Head = struct { seq: u64, hash: [32]u8 };

pub const Options = struct {
    computer: []const u8,
    chain: Chain = .record,
    head: ?Head = null,
    blobs: ?Blobs = null,
};

/// Memory for parsing one line. Every line is parsed in it and it is reset after each.
const parse_memory = 8 * format.max_line;

const max_missing = 1024;

pub fn verify(segments: []const []const u8, opts: Options) Result {
    const mem = std.heap.page_allocator.alloc(u8, parse_memory) catch return .{ .problem = .{ .segment = 0, .offset = 0, .seq = 0, .what = "out of memory" } };
    defer std.heap.page_allocator.free(mem);
    var fba = std.heap.FixedBufferAllocator.init(mem);
    var v = Verifier{ .opts = opts, .fba = &fba };
    v.run(segments);
    return v.r;
}

const Verifier = struct {
    opts: Options,
    fba: *std.heap.FixedBufferAllocator,
    r: Result = .{},
    head_seen: bool = false,
    // Blobs missing from disk, and blobs a removal fact names. Bounded.
    missing: [max_missing][64]u8 = undefined,
    missing_at: [max_missing]Problem = undefined,
    n_missing: usize = 0,
    removed: [max_missing][64]u8 = undefined,
    n_removed: usize = 0,

    fn fail(v: *Verifier, seg: u32, off: u64, what: []const u8) void {
        if (v.r.problem == null) v.r.problem = .{ .segment = seg, .offset = off, .seq = v.r.last_seq + 1, .what = what };
    }

    fn run(v: *Verifier, segments: []const []const u8) void {
        const want_computer = format.computerId(v.opts.computer) orelse return v.fail(0, 0, "not a computer's name");
        for (segments, 0..) |bytes, i| {
            const n: u32 = @intCast(i + 1);
            v.r.segments = n;
            if (bytes.len < format.header_size) return v.fail(n, 0, "the segment is shorter than its header");
            const h = format.Header.decode(bytes[0..format.header_size]) catch |e| return v.fail(n, 0, switch (e) {
                error.BadMagic => "the segment does not start with SILOREC",
                error.BadVersion => "the segment is of a format version this verify does not read",
                error.BadChecksum => "the segment's header does not match its checksum",
                error.BadReserved => "a reserved byte of the header is not zero",
            });
            if (h.compression != 0) return v.fail(n, 0, "the segment is compressed, which this verify does not read");
            if (h.computer != want_computer) return v.fail(n, 0, "the segment belongs to another computer");
            if (h.segment != n) return v.fail(n, 0, "segments are missing or out of order");
            if (h.first_seq != v.r.last_seq + 1) return v.fail(n, 0, "the segment does not start at the line after the last one");
            if (!std.mem.eql(u8, &h.prev, &v.r.last_hash)) return v.fail(n, 0, "the segment does not follow the last line of the segment before");
            var frames = format.Frames{ .bytes = bytes };
            while (true) switch (frames.next()) {
                .end => break,
                .bad => |b| {
                    if (b.torn and i == segments.len - 1)
                        return v.fail(n, b.off, "the last line is unfinished: the recorder stopped while writing it, or it was changed");
                    return v.fail(n, b.off, b.why.text());
                },
                .line => |l| {
                    v.checkLine(n, l.off, l.payload) catch return v.fail(n, l.off, "a line is too deep or too large to read");
                    if (v.r.problem != null) return;
                },
            };
        }
        if (v.opts.head) |head| {
            if (head.seq > v.r.last_seq) {
                v.r.problem = .{ .segment = v.r.segments, .offset = 0, .seq = v.r.last_seq + 1, .what = "the record ends before the head's line: lines were removed at the end" };
                return;
            }
            if (!v.head_seen and head.seq != 0) return v.fail(v.r.segments, 0, "the head's line was not found");
        }
        for (v.missing[0..v.n_missing], v.missing_at[0..v.n_missing]) |m, at| {
            var named = false;
            for (v.removed[0..v.n_removed]) |r| if (std.mem.eql(u8, &r, &m)) {
                named = true;
            };
            if (!named) {
                v.r.problem = .{ .segment = at.segment, .offset = at.offset, .seq = at.seq, .what = "a blob a line points to is missing, and no fact says it was removed" };
                return;
            }
        }
    }

    fn checkLine(v: *Verifier, seg: u32, off: u64, payload: []const u8) !void {
        const seq = v.r.last_seq + 1;
        // The envelope's start is fixed byte for byte, so a line has one form only.
        var want: [128]u8 = undefined;
        const prefix = std.fmt.bufPrint(&want, "{{\"v\":1,\"seq\":{d},\"prev\":\"{s}\",\"time\":\"", .{ seq, format.hex(v.r.last_hash) }) catch unreachable;
        if (!std.mem.startsWith(u8, payload, prefix)) {
            var sb: [64]u8 = undefined;
            const seq_part = std.fmt.bufPrint(&sb, "{{\"v\":1,\"seq\":{d},\"prev\":\"", .{seq}) catch unreachable;
            if (std.mem.startsWith(u8, payload, seq_part))
                return v.fail(seg, off, "the line before it was changed, removed or moved");
            return v.fail(seg, off, "the line is not the next line of the chain");
        }
        if (!std.unicode.utf8ValidateSlice(payload)) return v.fail(seg, off, "the line is not UTF-8");
        v.fba.reset();
        const a = v.fba.allocator();
        const val = std.json.parseFromSliceLeaky(std.json.Value, a, payload, .{ .duplicate_field_behavior = .@"error", .max_value_len = format.max_line }) catch |e| switch (e) {
            error.OutOfMemory => return error.OutOfMemory,
            else => return v.fail(seg, off, "the line is not one JSON object"),
        };
        if (val != .object) return v.fail(seg, off, "the line is not one JSON object");
        const o = val.object;
        const computer = getStr(o, "computer") orelse return v.fail(seg, off, "the line names no computer");
        if (!std.mem.eql(u8, computer, v.opts.computer)) return v.fail(seg, off, "the line names another computer");
        const time = getStr(o, "time") orelse return v.fail(seg, off, "the line has no time");
        if (time.len != 30 or time[29] != 'Z') return v.fail(seg, off, "the line's time is not UTC in nanoseconds");
        const src = o.get("source") orelse return v.fail(seg, off, "the line has no source");
        if (src != .object or getStr(src.object, "id") == null) return v.fail(seg, off, "the line's source has no id");
        const is = getStr(o, "is") orelse return v.fail(seg, off, "the line does not say what it is");
        switch (v.opts.chain) {
            .record => {
                if (!std.mem.eql(u8, is, "fact") and !std.mem.eql(u8, is, "report")) return v.fail(seg, off, "a line of the record is a fact or a report");
                const kind = getStr(o, "kind") orelse return v.fail(seg, off, "the line has no kind");
                if (!line.isKind(kind)) return v.fail(seg, off, "the line's kind is not one of the nine");
                if (std.mem.eql(u8, kind, "control")) {
                    if (!std.mem.eql(u8, is, "fact")) return v.fail(seg, off, "a control line is a fact");
                    const au = o.get("author") orelse return v.fail(seg, off, "a control fact has no author");
                    if (au != .object) return v.fail(seg, off, "a control fact has no author");
                    const by = getStr(au.object, "by") orelse return v.fail(seg, off, "a control fact has no author");
                    if (!line.isAuthor(by)) return v.fail(seg, off, "a control fact's author is not one Silo knows");
                    if (std.mem.eql(u8, by, "you") and getStr(au.object, "person") == null) return v.fail(seg, off, "a control fact made by you names no person");
                    if (std.mem.eql(u8, getStr(o, "what") orelse "", "bulky-removed")) v.noteRemoved(o);
                }
            },
            .opinions => if (!std.mem.eql(u8, is, "opinion")) return v.fail(seg, off, "a line of the opinions is an opinion"),
        }
        if (v.opts.blobs) |b| v.checkBlobs(b, seg, off, val);
        if (v.opts.head) |h| if (h.seq == seq) {
            v.head_seen = true;
            if (!std.mem.eql(u8, &h.hash, &format.lineHash(payload))) return v.fail(seg, off, "the head's line was changed");
        };
        if (v.opts.head) |h| {
            if (seq > h.seq) v.r.after_head += 1;
        }
        v.r.lines += 1;
        v.r.last_seq = seq;
        v.r.last_hash = format.lineHash(payload);
    }

    fn noteRemoved(v: *Verifier, o: std.json.ObjectMap) void {
        const arr = o.get("blobs") orelse return;
        if (arr != .array) return;
        for (arr.array.items) |it| {
            if (it != .string or !isBlobRef(it.string)) continue;
            if (v.n_removed == max_missing) return;
            v.removed[v.n_removed] = it.string[7..71].*;
            v.n_removed += 1;
            v.r.blobs_removed += 1;
        }
    }

    fn checkBlobs(v: *Verifier, b: Blobs, seg: u32, off: u64, val: std.json.Value) void {
        switch (val) {
            .string => |s| if (isBlobRef(s)) {
                v.r.blobs_checked += 1;
                switch (b.check(b.ctx, s[7..71])) {
                    .whole => {},
                    .changed => v.fail(seg, off, "a blob's bytes do not match its hash"),
                    .missing => if (v.n_missing < max_missing) {
                        v.missing[v.n_missing] = s[7..71].*;
                        v.missing_at[v.n_missing] = .{ .segment = seg, .offset = off, .seq = v.r.last_seq + 1, .what = "" };
                        v.n_missing += 1;
                    } else v.fail(seg, off, "too many blobs are missing"),
                }
            },
            .array => |arr| for (arr.items) |it| v.checkBlobs(b, seg, off, it),
            .object => |o| {
                var it = o.iterator();
                while (it.next()) |e| v.checkBlobs(b, seg, off, e.value_ptr.*);
            },
            else => {},
        }
    }
};

fn getStr(o: std.json.ObjectMap, k: []const u8) ?[]const u8 {
    const x = o.get(k) orelse return null;
    return if (x == .string) x.string else null;
}

pub fn isBlobRef(s: []const u8) bool {
    if (s.len != 71 or !std.mem.startsWith(u8, s, "sha256:")) return false;
    for (s[7..]) |c| if (!std.ascii.isDigit(c) and !(c >= 'a' and c <= 'f')) return false;
    return true;
}

/// Reads the head file: {"v":1,"computer":"c1","seq":…,"hash":"…","time":"…"}
pub fn parseHead(gpa: std.mem.Allocator, bytes: []const u8, computer: []const u8) !Head {
    const p = try std.json.parseFromSlice(std.json.Value, gpa, bytes, .{});
    defer p.deinit();
    if (p.value != .object) return error.BadHead;
    const o = p.value.object;
    const c = getStr(o, "computer") orelse return error.BadHead;
    if (!std.mem.eql(u8, c, computer)) return error.BadHead;
    const seq = o.get("seq") orelse return error.BadHead;
    if (seq != .integer or seq.integer < 0) return error.BadHead;
    const h = getStr(o, "hash") orelse return error.BadHead;
    if (h.len != 64) return error.BadHead;
    var hash: [32]u8 = undefined;
    _ = std.fmt.hexToBytes(&hash, h) catch return error.BadHead;
    return .{ .seq = @intCast(seq.integer), .hash = hash };
}

/// Blobs in a folder: `blobs/<first two hex>/<hex>`.
pub const DirBlobs = struct {
    dir: std.fs.Dir,

    pub fn blobs(d: *DirBlobs) Blobs {
        return .{ .ctx = d, .check = check };
    }

    fn check(ctx: *anyopaque, hex: []const u8) BlobState {
        const d: *DirBlobs = @ptrCast(@alignCast(ctx));
        var pbuf: [80]u8 = undefined;
        const p = std.fmt.bufPrint(&pbuf, "{s}/{s}", .{ hex[0..2], hex }) catch unreachable;
        const f = d.dir.openFile(p, .{}) catch return .missing;
        defer f.close();
        var h = format.Sha256.init(.{});
        var buf: [64 * 1024]u8 = undefined;
        while (true) {
            const n = f.read(&buf) catch return .changed;
            if (n == 0) break;
            h.update(buf[0..n]);
        }
        const got = std.fmt.bytesToHex(h.finalResult(), .lower);
        return if (std.mem.eql(u8, &got, hex)) .whole else .changed;
    }
};

/// Reads every segment of `<dir>` in order into memory. The prototype reads whole segments; the
/// recorder's verify reads them in pieces of fixed size, which the frame walk already allows.
pub fn readSegments(gpa: std.mem.Allocator, dir: std.fs.Dir) !std.ArrayList([]u8) {
    var names: std.ArrayList(u32) = .empty;
    defer names.deinit(gpa);
    var it = dir.iterate();
    while (try it.next()) |e| if (format.parseSegmentName(e.name)) |n| try names.append(gpa, n);
    std.mem.sort(u32, names.items, {}, std.sort.asc(u32));
    var out: std.ArrayList([]u8) = .empty;
    for (names.items) |n| {
        var nb: [32]u8 = undefined;
        try out.append(gpa, try dir.readFileAlloc(gpa, format.segmentName(&nb, n), 1 << 30));
    }
    // A missing segment shows as a header whose number is not the expected one.
    return out;
}
