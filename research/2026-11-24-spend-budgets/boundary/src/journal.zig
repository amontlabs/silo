//! The spend journal: what each ledger spent, kept on disk so that a budget holds across a restart
//! of the boundary, a crash or a power cut (research/2026-11-25-durability-on-a-mac).
//!
//! Without it, the ledgers live in memory and a boundary that starts again starts every budget at
//! zero: a computer that had reached its $1 spent another dollar after a kill (restart.sh).
//!
//! One line per event, appended and made durable before the call it describes goes on:
//!
//!     b <boot>                                      the boundary started
//!     s <boot> <id> <reserve> <t> <ledger>...       a call left, holding `reserve` dollars
//!     e <boot> <id> <cost> <t>                      the call settled at `cost`
//!
//! Fields are separated by tabs, and each line ends with the CRC-32 of what comes before it.
//! At start, every settled call is put back on its ledgers, at its time (for rate limits). A call
//! that left and never settled, because the boundary stopped while it ran, counts at its reserve:
//! the provider may have billed it, and the reserve is the most it can cost (`max_tokens` of
//! output; under `reserve_typical` the typical output, which can be less).
//!
//! A power cut can leave the line being written in part, or zeros where a sector did not arrive
//! (research/2026-11-25-record-head-off-the-mac). That line was never acknowledged, since the
//! call waits for its flush. So lines that do not check at the end of the file are cut; a line that
//! does not check followed by one that does is damage, and the boundary does not start.
const std = @import("std");
const builtin = @import("builtin");
const posix = std.posix;
const Allocator = std.mem.Allocator;

/// How a line is made durable. `full` is the default and the only one that keeps a line through
/// a power cut on a Mac; the others exist to measure what it costs (the Mac kit's test 40).
pub const Flush = enum { full, barrier, fsync, none };

pub fn flushFromEnv() Flush {
    const v = posix.getenv("SILO_FLUSH") orelse return .full;
    return std.meta.stringToEnum(Flush, v) orelse .full;
}

/// The file's data and size made durable. On a Mac, fsync(2) only hands the data to the drive,
/// which may keep it in its cache and write it out of order; F_FULLFSYNC flushes that cache.
/// Zig's std.posix.fsync and fdatasync call plain fsync on macOS, so this asks for F_FULLFSYNC
/// itself, and falls back to fsync on a volume that refuses it (SMB), as Go's File.Sync does.
pub fn durable(fd: posix.fd_t, how: Flush) !void {
    switch (how) {
        .none => return,
        .fsync => return posix.fsync(fd),
        .barrier, .full => {},
    }
    if (comptime builtin.os.tag.isDarwin()) {
        const cmd: c_int = if (how == .full) std.c.F.FULLFSYNC else 85; // F_BARRIERFSYNC
        const rc = std.c.fcntl(fd, cmd);
        if (rc == 0) return;
        switch (posix.errno(rc)) {
            .OPNOTSUPP, .INVAL => return posix.fsync(fd),
            else => |e| return posix.unexpectedErrno(e),
        }
    }
    return posix.fdatasync(fd);
}

pub const Spent = struct { t: f64, cost: f64 };

/// What the journal says one ledger spent before this start.
pub const Carry = struct {
    settled: f64 = 0,
    /// Calls that left and never settled, counted at their reserve.
    interrupted: u32 = 0,
    calls: u32 = 0,
    history: std.ArrayList(Spent) = .empty,
};

const Open = struct { reserve: f64, t: f64, names: []const []const u8 };

pub const Journal = struct {
    gpa: Allocator,
    file: std.fs.File,
    boot: u64,
    how: Flush,
    mu: std.Thread.Mutex = .{},
    carried: std.StringHashMapUnmanaged(Carry) = .empty,
    /// Bytes cut from the end at start, as a power cut left them.
    cut: u64 = 0,
    /// Interrupted calls found at start, over all ledgers.
    interrupted: u32 = 0,

    pub fn open(gpa: Allocator, path: []const u8, how: Flush) !*Journal {
        const cwd = std.fs.cwd();
        const created = blk: {
            cwd.access(path, .{}) catch break :blk true;
            break :blk false;
        };
        const file = try cwd.createFile(path, .{ .read = true, .truncate = false });
        errdefer file.close();
        const j = try gpa.create(Journal);
        j.* = .{ .gpa = gpa, .file = file, .boot = 1, .how = how };
        const bytes = try file.readToEndAlloc(gpa, 1 << 30);
        defer gpa.free(bytes);
        const keep = try j.replay(bytes);
        if (keep < bytes.len) {
            j.cut = bytes.len - keep;
            try file.setEndPos(keep);
            try durable(file.handle, how);
        }
        try file.seekFromEnd(0);
        if (created) {
            // The file's name must survive too.
            const dir_path = std.fs.path.dirname(path) orelse ".";
            var dir = try cwd.openDir(dir_path, .{ .iterate = true });
            defer dir.close();
            try durable(dir.fd, how);
        }
        var buf: [64]u8 = undefined;
        try j.put(try std.fmt.bufPrint(&buf, "b\t{d}", .{j.boot}));
        return j;
    }

    /// Puts the journal's lines back into `carried` and returns how many bytes of it hold.
    fn replay(j: *Journal, bytes: []const u8) !usize {
        var open_calls: std.AutoHashMapUnmanaged(u128, Open) = .empty;
        defer open_calls.deinit(j.gpa);
        var good_end: usize = 0;
        var bad_at: ?usize = null;
        var pos: usize = 0;
        while (pos < bytes.len) {
            const nl = std.mem.indexOfScalarPos(u8, bytes, pos, '\n') orelse {
                if (bad_at == null) bad_at = pos;
                break;
            };
            const line = bytes[pos..nl];
            const ok = j.apply(line, &open_calls) catch false;
            if (ok) {
                if (bad_at != null) return error.JournalDamaged;
                good_end = nl + 1;
            } else if (bad_at == null) bad_at = pos;
            pos = nl + 1;
        }
        // Calls that never settled count at their reserve.
        var it = open_calls.valueIterator();
        while (it.next()) |o| {
            for (o.names) |n| {
                const c = try j.carry(n);
                c.settled += o.reserve;
                c.interrupted += 1;
                c.calls += 1;
                try c.history.append(j.gpa, .{ .t = o.t, .cost = o.reserve });
            }
            j.interrupted += 1;
        }
        return good_end;
    }

    fn carry(j: *Journal, name: []const u8) !*Carry {
        const gop = try j.carried.getOrPut(j.gpa, name);
        if (!gop.found_existing) {
            gop.key_ptr.* = try j.gpa.dupe(u8, name);
            gop.value_ptr.* = .{};
        }
        return gop.value_ptr;
    }

    fn apply(j: *Journal, line: []const u8, open_calls: *std.AutoHashMapUnmanaged(u128, Open)) !bool {
        const tab = std.mem.lastIndexOfScalar(u8, line, '\t') orelse return false;
        const crc = std.fmt.parseInt(u32, line[tab + 1 ..], 16) catch return false;
        const body = line[0..tab];
        if (std.hash.Crc32.hash(body) != crc) return false;
        var f = std.mem.splitScalar(u8, body, '\t');
        const kind = f.next() orelse return false;
        const boot = try std.fmt.parseInt(u64, f.next() orelse return false, 10);
        if (boot >= j.boot) j.boot = boot + 1;
        if (std.mem.eql(u8, kind, "b")) return true;
        const id = try std.fmt.parseInt(u64, f.next() orelse return false, 10);
        const key = (@as(u128, boot) << 64) | id;
        const amount = try std.fmt.parseFloat(f64, f.next() orelse return false);
        const t = try std.fmt.parseFloat(f64, f.next() orelse return false);
        if (std.mem.eql(u8, kind, "s")) {
            var names: std.ArrayList([]const u8) = .empty;
            while (f.next()) |n| try names.append(j.gpa, try j.gpa.dupe(u8, n));
            try open_calls.put(j.gpa, key, .{ .reserve = amount, .t = t, .names = try names.toOwnedSlice(j.gpa) });
            return true;
        }
        if (std.mem.eql(u8, kind, "e")) {
            const o = open_calls.fetchRemove(key) orelse return true; // settled twice: once counts
            for (o.value.names) |n| {
                const c = try j.carry(n);
                c.settled += amount;
                c.calls += 1;
                try c.history.append(j.gpa, .{ .t = t, .cost = amount });
            }
            return true;
        }
        return false;
    }

    /// Appends one line and makes it durable before returning.
    fn put(j: *Journal, body: []const u8) !void {
        var buf: [4096]u8 = undefined;
        const line = try std.fmt.bufPrint(&buf, "{s}\t{x:0>8}\n", .{ body, std.hash.Crc32.hash(body) });
        try j.file.writeAll(line);
        try durable(j.file.handle, j.how);
    }

    /// A call is about to leave: it holds `reserve` on each named ledger until it settles.
    pub fn started(j: *Journal, id: u64, reserve: f64, t: f64, names: []const []const u8) !void {
        j.mu.lock();
        defer j.mu.unlock();
        var buf: [3072]u8 = undefined;
        var w = std.Io.Writer.fixed(&buf);
        try w.print("s\t{d}\t{d}\t{d:.9}\t{d:.3}", .{ j.boot, id, reserve, t });
        for (names) |n| try w.print("\t{s}", .{n});
        try j.put(w.buffered());
    }

    pub fn settled(j: *Journal, id: u64, cost: f64, t: f64) !void {
        j.mu.lock();
        defer j.mu.unlock();
        var buf: [256]u8 = undefined;
        try j.put(try std.fmt.bufPrint(&buf, "e\t{d}\t{d}\t{d:.9}\t{d:.3}", .{ j.boot, id, cost, t }));
    }
};

test "a journal gives back what was spent, counts an unsettled call at its reserve, cuts a torn tail" {
    const gpa = std.testing.allocator;
    var arena = std.heap.ArenaAllocator.init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    const path = try tmp.dir.realpathAlloc(a, ".");
    const p = try std.fs.path.join(a, &.{ path, "spent.journal" });

    var j = try Journal.open(a, p, .full);
    try j.started(1, 0.5, 10, &.{ "main", "person:ann" });
    try j.settled(1, 0.25, 11);
    try j.started(2, 0.75, 12, &.{"main"});
    j.file.close();
    // A power cut while a third line was written: part of it, then zeros.
    {
        const f = try std.fs.cwd().openFile(p, .{ .mode = .read_write });
        defer f.close();
        try f.seekFromEnd(0);
        try f.writeAll("s\t1\t3\t0.5" ++ "\x00" ** 40);
    }
    var k = try Journal.open(a, p, .full);
    try std.testing.expectEqual(@as(u64, 2), k.boot);
    try std.testing.expectEqual(@as(u64, 49), k.cut);
    const main = k.carried.get("main").?;
    try std.testing.expectApproxEqAbs(@as(f64, 1.0), main.settled, 1e-9);
    try std.testing.expectEqual(@as(u32, 1), main.interrupted);
    try std.testing.expectApproxEqAbs(@as(f64, 0.25), k.carried.get("person:ann").?.settled, 1e-9);
    k.file.close();

    // A bad line with a good one after it is damage, not a torn tail.
    {
        const f = try std.fs.cwd().openFile(p, .{ .mode = .read_write });
        defer f.close();
        try f.seekTo(3);
        try f.writeAll("9");
    }
    try std.testing.expectError(error.JournalDamaged, Journal.open(a, p, .full));
}
