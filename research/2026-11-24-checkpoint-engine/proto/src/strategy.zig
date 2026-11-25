//! Four ways to take a checkpoint of a disk image that a running computer writes.
//!
//! full   clone the live image whole (APFS clonefile, Linux FICLONE). Every write waits for the
//!        whole clone, which grows with the pieces of the image (#347, #352).
//! range  Linux only. Clone the last checkpoint (nobody writes it), then clone into it only the
//!        ranges of the live image whose blocks differ, found from the two block maps; a last
//!        pass with the computer paused catches what changed meanwhile.
//! cbw    When Silo sees the computer's writes (its own virtio-blk in libkrun, or a disk it serves
//!        over NBD to Apple's framework): a log marks each block written. A checkpoint clones the
//!        last checkpoint, swaps the log during a pause of a few microseconds, and copies the
//!        marked blocks into the clone; a write to a marked block not yet copied copies it first.
//!        Uses only clonefile, pread and pwrite, so it runs on a Mac as it is.
//! btrfs  Linux only. The image lives in a subvolume; a checkpoint is a read-only snapshot of it,
//!        the analogue of an APFS volume snapshot.
const std = @import("std");
const posix = std.posix;
const sys = @import("sys.zig");
const W = @import("writer.zig");
const BS = W.BS;

pub const Kind = enum { none, full, range, cbw, btrfs, nbd, delta };

pub const Result = struct { op_ns: u64, clone_end: u64 = 0, pause_at: u64 = 0, publish_at: u64 = 0, pause_ns: u64 = 0, ranges: u64 = 0, bytes_copied: u64 = 0, cow_copies: u64 = 0 };

/// The ranges of the file where a and b map to different physical blocks (or one has none).
pub fn diffMaps(gpa: std.mem.Allocator, a: []const sys.Extent, b: []const sys.Extent, size: u64, out: *std.ArrayList([2]u64)) !void {
    var i: usize = 0;
    var j: usize = 0;
    var pos: u64 = 0;
    while (pos < size) {
        while (i < a.len and a[i].logical + a[i].len <= pos) i += 1;
        while (j < b.len and b[j].logical + b[j].len <= pos) j += 1;
        const ea: ?sys.Extent = if (i < a.len and a[i].logical <= pos) a[i] else null;
        const eb: ?sys.Extent = if (j < b.len and b[j].logical <= pos) b[j] else null;
        var next: u64 = size;
        if (ea) |e| next = @min(next, e.logical + e.len) else if (i < a.len) next = @min(next, a[i].logical);
        if (eb) |e| next = @min(next, e.logical + e.len) else if (j < b.len) next = @min(next, b[j].logical);
        const pa: ?u64 = if (ea) |e| e.physical + (pos - e.logical) else null;
        const pb: ?u64 = if (eb) |e| e.physical + (pos - e.logical) else null;
        const same = if (pa != null and pb != null) pa.? == pb.? else (pa == null and pb == null);
        if (!same) {
            if (out.items.len > 0 and out.items[out.items.len - 1][1] == pos) {
                out.items[out.items.len - 1][1] = next;
            } else try out.append(gpa, .{ pos, next });
        }
        pos = next;
    }
}

pub fn mapOf(gpa: std.mem.Allocator, path: []const u8, size: u64) !std.ArrayList(sys.Extent) {
    const fd = try sys.openRead(path);
    defer posix.close(fd);
    var l: std.ArrayList(sys.Extent) = .empty;
    try sys.extents(gpa, fd, 0, size, &l);
    return l;
}

pub fn publish(tmp: [:0]const u8, dst: [:0]const u8, dir: []const u8) !void {
    const fd = try posix.open(tmp, .{ .ACCMODE = .RDONLY }, 0);
    try sys.fullSync(fd);
    posix.close(fd);
    try posix.rename(tmp, dst);
    try sys.syncDir(dir);
}

pub fn full(live: [:0]const u8, tmp: [:0]const u8, dst: [:0]const u8, dir: []const u8) !Result {
    const t0 = sys.now();
    try sys.cloneFile(live, tmp);
    const t1 = sys.now();
    try publish(tmp, dst, dir);
    return .{ .op_ns = t1 - t0 };
}

pub fn range(gpa: std.mem.Allocator, w: *W.Writer, live: [:0]const u8, prev: [:0]const u8, tmp: [:0]const u8, dst: [:0]const u8, dir: []const u8, size: u64) !Result {
    const t0 = sys.now();
    try sys.cloneFile(prev, tmp);
    const clone_end = sys.now();
    const lfd = try sys.openRead(live);
    defer posix.close(lfd);
    const tfd = try posix.open(tmp, .{ .ACCMODE = .RDWR }, 0);
    defer posix.close(tfd);
    var r: Result = .{ .op_ns = 0, .clone_end = clone_end };
    var pass: u32 = 0;
    while (pass < 2) : (pass += 1) {
        if (pass == 1) w.pause();
        const p0 = sys.now();
        if (pass == 1) r.pause_at = p0;
        var la = try mapOf(gpa, live, size);
        defer la.deinit(gpa);
        var lb: std.ArrayList(sys.Extent) = .empty;
        defer lb.deinit(gpa);
        try sys.extents(gpa, tfd, 0, size, &lb);
        var d: std.ArrayList([2]u64) = .empty;
        defer d.deinit(gpa);
        try diffMaps(gpa, la.items, lb.items, size, &d);
        for (d.items) |x| try sys.cloneRange(lfd, tfd, x[0], x[1] - x[0]);
        r.ranges += d.items.len;
        if (pass == 1) {
            w.unpause();
            r.pause_ns = sys.now() - p0;
        }
    }
    r.op_ns = sys.now() - t0;
    r.publish_at = sys.now();
    try publish(tmp, dst, dir);
    return r;
}

/// The copy-before-write state, shared by the writer's hook and the copier.
pub const Cbw = struct {
    mu: std.Thread.Mutex = .{},
    cur: []u64, // blocks written since the last checkpoint
    frozen: []u64, // blocks to copy into the checkpoint being made
    active: bool = false,
    live_fd: posix.fd_t,
    tmp_fd: posix.fd_t = -1,
    buf: []align(4096) u8, // one block, for the writer's own copies
    cbuf: []align(4096) u8, // the copier's run
    cow_copies: u64 = 0,
    bytes: u64 = 0,
    /// A checkpoint as a delta: a new sparse file holding only the blocks copied, with the map of
    /// those blocks after them, instead of a clone of the last checkpoint. Nothing is cloned while
    /// the computer runs; reading a checkpoint means applying the deltas since the first one.
    delta: bool = false,
    size: u64 = 0,
    map: []u64 = &.{},

    pub fn init(gpa: std.mem.Allocator, live: []const u8, blocks: u64) !*Cbw {
        const c = try gpa.create(Cbw);
        const words = (blocks + 63) / 64;
        c.* = .{
            .cur = try gpa.alloc(u64, words),
            .frozen = try gpa.alloc(u64, words),
            .live_fd = try sys.openImage(live, false),
            .buf = try gpa.alignedAlloc(u8, .fromByteUnits(4096), BS),
            .cbuf = try gpa.alignedAlloc(u8, .fromByteUnits(4096), 64 * BS),
        };
        @memset(c.cur, 0);
        @memset(c.frozen, 0);
        return c;
    }

    pub fn hook(c: *Cbw) W.Hook {
        return .{ .ctx = c, .before = before };
    }

    pub fn before(ctx: *anyopaque, b: u32) void {
        const c: *Cbw = @ptrCast(@alignCast(ctx));
        const word = b / 64;
        const bit = @as(u64, 1) << @intCast(b % 64);
        c.mu.lock();
        defer c.mu.unlock();
        c.cur[word] |= bit;
        if (c.active and c.frozen[word] & bit != 0) {
            _ = posix.pread(c.live_fd, c.buf, @as(u64, b) * BS) catch unreachable;
            _ = posix.pwrite(c.tmp_fd, c.buf, @as(u64, b) * BS) catch unreachable;
            c.frozen[word] &= ~bit;
            c.cow_copies += 1;
            c.bytes += BS;
        }
    }

    /// Copies the next run of up to 64 marked blocks at or after `from`; returns where to go on,
    /// or null when nothing is left.
    fn copyRun(c: *Cbw, from: u64, blocks: u64) ?u64 {
        c.mu.lock();
        defer c.mu.unlock();
        var b = from;
        while (b < blocks) {
            const word = c.frozen[b / 64] >> @intCast(b % 64);
            if (word == 0) {
                b = (b / 64 + 1) * 64;
                continue;
            }
            b += @ctz(word);
            break;
        }
        if (b >= blocks) return null;
        var n: u64 = 0;
        while (n < 64 and b + n < blocks and c.frozen[(b + n) / 64] & (@as(u64, 1) << @intCast((b + n) % 64)) != 0) n += 1;
        const len = n * BS;
        _ = posix.pread(c.live_fd, c.cbuf[0..len], b * BS) catch unreachable;
        _ = posix.pwrite(c.tmp_fd, c.cbuf[0..len], b * BS) catch unreachable;
        var k: u64 = 0;
        while (k < n) : (k += 1) c.frozen[(b + k) / 64] &= ~(@as(u64, 1) << @intCast((b + k) % 64));
        c.bytes += len;
        return b + n;
    }

    /// Step 1: clone the last checkpoint, which nobody writes, as the new one's start.
    pub fn prepare(c: *Cbw, prev: [:0]const u8, tmp: [:0]const u8) !void {
        if (c.delta) {
            const fd = try posix.open(tmp, .{ .ACCMODE = .RDWR, .CREAT = true, .EXCL = true }, 0o644);
            try posix.ftruncate(fd, c.size);
            posix.close(fd);
        } else try sys.cloneFile(prev, tmp);
        c.tmp_fd = try sys.openImage(tmp, false);
        c.cow_copies = 0;
        c.bytes = 0;
    }

    /// Step 2, the moment of the checkpoint: called with no write in flight. Swaps the logs.
    pub fn swap(c: *Cbw) void {
        c.mu.lock();
        const t = c.frozen;
        c.frozen = c.cur;
        c.cur = t; // all clear: the last checkpoint copied every block of it
        c.active = true;
        if (c.delta) @memcpy(c.map, c.frozen);
        c.mu.unlock();
    }

    /// Step 3: copy what the log marked, while the computer writes on.
    pub fn drain(c: *Cbw, blocks: u64) void {
        var at: u64 = 0;
        while (c.copyRun(at, blocks)) |next| at = next;
        c.mu.lock();
        c.active = false;
        c.mu.unlock();
        if (c.delta) _ = posix.pwrite(c.tmp_fd, std.mem.sliceAsBytes(c.map), c.size) catch unreachable;
        posix.close(c.tmp_fd);
        c.tmp_fd = -1;
    }

    pub fn checkpoint(c: *Cbw, w: *W.Writer, prev: [:0]const u8, tmp: [:0]const u8, dst: [:0]const u8, dir: []const u8, blocks: u64) !Result {
        const t0 = sys.now();
        try c.prepare(prev, tmp);
        const clone_end = sys.now();
        // The computer stops between two writes, as a pause of the virtual machine does.
        w.pause();
        const p0 = sys.now();
        c.swap();
        const pause_ns = sys.now() - p0;
        w.unpause();
        c.drain(blocks);
        const op = sys.now() - t0;
        const publish_at = sys.now();
        try publish(tmp, dst, dir);
        return .{ .op_ns = op, .publish_at = publish_at, .clone_end = clone_end, .pause_at = p0, .pause_ns = pause_ns, .bytes_copied = c.bytes, .cow_copies = c.cow_copies };
    }
};

/// The map of blocks a delta holds.
pub fn readMap(gpa: std.mem.Allocator, path: []const u8, size: u64) ![]u64 {
    const words = (size / BS + 63) / 64;
    const m = try gpa.alloc(u64, words);
    const fd = try sys.openRead(path);
    defer posix.close(fd);
    _ = try posix.pread(fd, std.mem.sliceAsBytes(m), size);
    return m;
}

/// Thinning a delta: the blocks it holds that the next one lacks move into the next one, then it
/// goes. The next checkpoint, read as a disk, is the same before and after.
pub fn mergeInto(gpa: std.mem.Allocator, from: [:0]const u8, into: [:0]const u8, size: u64) !u64 {
    const a = try readMap(gpa, from, size);
    defer gpa.free(a);
    const b = try readMap(gpa, into, size);
    defer gpa.free(b);
    const ffd = try sys.openRead(from);
    defer posix.close(ffd);
    const ifd = try posix.open(into, .{ .ACCMODE = .RDWR }, 0);
    defer posix.close(ifd);
    var buf: [BS]u8 = undefined;
    var moved: u64 = 0;
    for (a, b, 0..) |wa, *wb, wi| {
        var m = wa & ~wb.*;
        while (m != 0) {
            const bit = @ctz(m);
            m &= m - 1;
            const off = (wi * 64 + bit) * BS;
            _ = try posix.pread(ffd, &buf, off);
            _ = try posix.pwrite(ifd, &buf, off);
            moved += 1;
        }
        wb.* |= wa;
    }
    _ = try posix.pwrite(ifd, std.mem.sliceAsBytes(b), size);
    try sys.fullSync(ifd);
    try posix.unlink(from);
    return moved;
}

/// A checkpoint read as a whole disk: the first checkpoint, with every delta up to it applied.
pub fn materialize(gpa: std.mem.Allocator, base: [:0]const u8, deltas: []const [:0]const u8, out: [:0]const u8, size: u64) !void {
    try sys.cloneFile(base, out);
    const ofd = try posix.open(out, .{ .ACCMODE = .RDWR }, 0);
    defer posix.close(ofd);
    var buf: [BS]u8 = undefined;
    for (deltas) |d| {
        const m = try readMap(gpa, d, size);
        defer gpa.free(m);
        const dfd = try sys.openRead(d);
        defer posix.close(dfd);
        for (m, 0..) |w, wi| {
            var x = w;
            while (x != 0) {
                const bit = @ctz(x);
                x &= x - 1;
                const off = (wi * 64 + bit) * BS;
                _ = try posix.pread(dfd, &buf, off);
                _ = try posix.pwrite(ofd, &buf, off);
            }
        }
    }
}

pub fn btrfs(subvol: []const u8, snaps: []const u8, name: []const u8) !Result {
    const t0 = sys.now();
    try sys.btrfsSnapshot(subvol, snaps, name, true);
    return .{ .op_ns = sys.now() - t0 };
}
