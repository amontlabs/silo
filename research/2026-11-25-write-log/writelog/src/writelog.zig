//! Silo's side of libkrun's write hook: the write log and `delta` checkpoints of a running disk.
//!
//! libkrun's block worker calls `silo_write_hook(user, offset, len)` before each write,
//! write-zeroes and discard of main's disk, with a range it has checked to lie inside the disk
//! (the libkrun patch in `../libkrun/`). The log marks the 4 KiB blocks of the range in a bitmap of
//! blocks written since the last checkpoint. A checkpoint is #365's `delta`:
//!
//!   1. `begin`: a new sparse file, the size of the disk, is ready to take blocks.
//!   2. `swap`, the checkpoint's moment: the bitmap of blocks written is swapped for an empty one,
//!      under the log's lock. It takes well under a microsecond (#365).
//!   3. `drain`: the blocks the old bitmap marks are copied from the live disk into the file, while
//!      main writes on. A write to a marked block not yet copied copies it first, in the hook,
//!      so the file gets the block as it was at the moment.
//!   4. The bitmap is written after the blocks, at offset `size`: it is the checkpoint's map and its
//!      `writes`, the exact set of blocks that changed since the last checkpoint.
//!
//! A checkpoint read as a disk is the first full copy with every delta up to it laid over it in
//! order, as #365's `materialize` does.
//!
//! The write in flight. The hook runs before the write lands, and libkrun gives no call after it.
//! A swap can fall between the two. libkrun's block worker changes the disk from one thread, one
//! request at a time, so at most one write is in flight at a swap: the last range the hook saw.
//! The swap marks that range again in the new bitmap. Without it, a checkpoint taken while that
//! write was in flight can copy the block before the write lands, the write never marks the block
//! for the next checkpoint, and the next checkpoint misses a write that completed long before its
//! moment (the test "a write in flight at a swap is in the next checkpoint" shows it).
//!
//! A copy that fails fails the checkpoint, never main's write: `drain` returns error.CopyFailed and
//! the checkpoint is not published.
const std = @import("std");
const linux = std.os.linux;
const fd_t = linux.fd_t;

/// Zig 0.17's std has no pread, pwrite or open outside `std.Io`; the trial calls Linux directly.
/// On a Mac the helper calls libc's, with the same arguments.
pub const sys = struct {
    fn check(rc: usize) !usize {
        return if (linux.errno(rc) == .SUCCESS) rc else error.SystemCall;
    }
    pub fn pread(fd: fd_t, buf: []u8, off: u64) !usize {
        return check(linux.pread(fd, buf.ptr, buf.len, @intCast(off)));
    }
    pub fn pwrite(fd: fd_t, buf: []const u8, off: u64) !usize {
        return check(linux.pwrite(fd, buf.ptr, buf.len, @intCast(off)));
    }
    pub fn open(path: []const u8, flags: linux.O, mode: linux.mode_t) !fd_t {
        var z: [256:0]u8 = undefined;
        if (path.len >= z.len) return error.NameTooLong;
        @memcpy(z[0..path.len], path);
        z[path.len] = 0;
        return @intCast(try check(linux.open(&z, flags, mode)));
    }
    pub fn mkdir(path: []const u8) !void {
        var z: [256:0]u8 = undefined;
        @memcpy(z[0..path.len], path);
        z[path.len] = 0;
        const rc = linux.mkdir(&z, 0o755);
        if (linux.errno(rc) != .SUCCESS and linux.errno(rc) != .EXIST) return error.SystemCall;
    }
    pub fn ftruncate(fd: fd_t, size: u64) !void {
        _ = try check(linux.ftruncate(fd, @intCast(size)));
    }
    pub fn fsync(fd: fd_t) !void {
        _ = try check(linux.fsync(fd));
    }
    pub fn close(fd: fd_t) void {
        _ = linux.close(fd);
    }
};

/// A lock held for microseconds at most (one run of 64 blocks copied).
pub const Mutex = struct {
    m: std.atomic.Mutex = .unlocked,
    pub fn lock(self: *Mutex) void {
        while (!self.m.tryLock()) std.atomic.spinLoopHint();
    }
    pub fn unlock(self: *Mutex) void {
        self.m.unlock();
    }
};

pub const BS: u64 = 4096;

/// libkrun's `krun_block_device_set_write_hook` takes a function of this type.
pub const Hook = *const fn (?*const anyopaque, u64, u64) callconv(.c) i32;

const EINVAL: i32 = 22;

pub const Stats = struct {
    /// Blocks the checkpoint holds: those written since the last checkpoint.
    blocks: u64 = 0,
    /// Of those, the ones the hook copied before a write, instead of the copier.
    copied_before_write: u64 = 0,
};

pub const Log = struct {
    mu: Mutex = .{},
    size: u64,
    blocks: u64,
    /// Blocks written since the last checkpoint's moment.
    cur: []u64,
    /// Blocks of the checkpoint being made that are not copied yet.
    frozen: []u64,
    /// The checkpoint's map: `frozen` as it was at the moment.
    map: []u64,
    /// The last range the hook saw: the write that may be in flight.
    last: ?[2]u64 = null,
    active: bool = false,
    failed: bool = false,
    live_fd: fd_t,
    delta_fd: fd_t = -1,
    buf: []u8,
    run: []u8,
    stats: Stats = .{},
    /// Tests only: set to false to show what goes wrong without the in-flight rule.
    remark_in_flight: bool = true,

    pub fn init(gpa: std.mem.Allocator, live_fd: fd_t, size: u64) !*Log {
        if (size == 0 or size % BS != 0) return error.SizeNotBlockMultiple;
        const blocks = size / BS;
        const words = (blocks + 63) / 64;
        const l = try gpa.create(Log);
        l.* = .{
            .size = size,
            .blocks = blocks,
            .cur = try gpa.alloc(u64, words),
            .frozen = try gpa.alloc(u64, words),
            .map = try gpa.alloc(u64, words),
            .live_fd = live_fd,
            .buf = try gpa.alloc(u8, BS),
            .run = try gpa.alloc(u8, 64 * BS),
        };
        @memset(l.cur, 0);
        @memset(l.frozen, 0);
        @memset(l.map, 0);
        return l;
    }

    pub fn deinit(l: *Log, gpa: std.mem.Allocator) void {
        gpa.free(l.cur);
        gpa.free(l.frozen);
        gpa.free(l.map);
        gpa.free(l.buf);
        gpa.free(l.run);
        gpa.destroy(l);
    }

    /// The user pointer to give libkrun with `hook`.
    pub fn user(l: *Log) ?*const anyopaque {
        return l;
    }

    pub const hook: Hook = silo_write_hook;

    fn bit(b: u64) u64 {
        return @as(u64, 1) << @intCast(b % 64);
    }

    fn copyBlock(l: *Log, b: u64, buf: []u8) bool {
        const off = b * BS;
        const n = sys.pread(l.live_fd, buf, off) catch return false;
        if (n != buf.len) return false;
        const w = sys.pwrite(l.delta_fd, buf, off) catch return false;
        return w == buf.len;
    }

    /// Called with the lock held.
    fn mark(l: *Log, first: u64, last: u64) void {
        var b = first;
        while (b <= last) : (b += 1) {
            const w = b / 64;
            l.cur[w] |= bit(b);
            if (l.active and l.frozen[w] & bit(b) != 0) {
                if (l.copyBlock(b, l.buf)) {
                    l.stats.copied_before_write += 1;
                } else l.failed = true;
                l.frozen[w] &= ~bit(b);
            }
        }
    }

    /// The hook's work. Returns 0, or a negative errno for a range outside the disk, which
    /// libkrun has already refused; the check is here because this code is Silo's.
    pub fn before(l: *Log, offset: u64, len: u64) i32 {
        if (len == 0) return 0;
        const end = std.math.add(u64, offset, len) catch return -EINVAL;
        if (end > l.size) return -EINVAL;
        const first = offset / BS;
        const last = (end - 1) / BS;
        l.mu.lock();
        defer l.mu.unlock();
        l.mark(first, last);
        l.last = .{ first, last };
        return 0;
    }

    /// Step 1. `delta_fd` is a new file, open for writing, of the disk's size.
    pub fn begin(l: *Log, delta_fd: fd_t) void {
        l.mu.lock();
        defer l.mu.unlock();
        std.debug.assert(!l.active);
        l.delta_fd = delta_fd;
        l.failed = false;
        l.stats = .{};
    }

    /// Step 2, the moment. See the top of the file for why the last range is marked again.
    pub fn swapLocked(l: *Log) void {
        std.debug.assert(!l.active and l.delta_fd >= 0);
        const t = l.frozen;
        l.frozen = l.cur;
        l.cur = t; // empty: the last drain copied every block of it
        @memcpy(l.map, l.frozen);
        l.active = true;
        if (l.remark_in_flight) {
            if (l.last) |r| {
                var b = r[0];
                while (b <= r[1]) : (b += 1) l.cur[b / 64] |= bit(b);
            }
        }
    }

    pub fn swap(l: *Log) void {
        l.mu.lock();
        defer l.mu.unlock();
        l.swapLocked();
    }

    /// Copies the next run of up to 64 marked blocks at or after `from`; returns where to go on,
    /// or null when none is left. Holds the lock for one run, so a write waits at most that long.
    fn copyRun(l: *Log, from: u64) ?u64 {
        l.mu.lock();
        defer l.mu.unlock();
        var b = from;
        while (b < l.blocks) {
            const word = l.frozen[b / 64] >> @intCast(b % 64);
            if (word == 0) {
                b = (b / 64 + 1) * 64;
                continue;
            }
            b += @ctz(word);
            break;
        }
        if (b >= l.blocks) return null;
        var n: u64 = 0;
        while (n < 64 and b + n < l.blocks and l.frozen[(b + n) / 64] & bit(b + n) != 0) n += 1;
        const len = n * BS;
        const ok = blk: {
            const r = sys.pread(l.live_fd, l.run[0..len], b * BS) catch break :blk false;
            if (r != len) break :blk false;
            const w = sys.pwrite(l.delta_fd, l.run[0..len], b * BS) catch break :blk false;
            break :blk w == len;
        };
        if (!ok) l.failed = true;
        var k: u64 = 0;
        while (k < n) : (k += 1) l.frozen[(b + k) / 64] &= ~bit(b + k);
        return b + n;
    }

    /// Steps 3 and 4. The caller publishes the file only if this returns without error.
    pub fn drain(l: *Log) !Stats {
        var at: u64 = 0;
        while (l.copyRun(at)) |next| at = next;
        l.mu.lock();
        l.active = false;
        const failed = l.failed;
        var stats = l.stats;
        const fd = l.delta_fd;
        l.delta_fd = -1;
        l.mu.unlock();
        for (l.map) |w| stats.blocks += @popCount(w);
        if (failed) return error.CopyFailed;
        const bytes = std.mem.sliceAsBytes(l.map);
        if (try sys.pwrite(fd, bytes, l.size) != bytes.len) return error.CopyFailed;
        try sys.fsync(fd);
        return stats;
    }
};

/// The function given to libkrun, with the `Log` as its user pointer.
pub export fn silo_write_hook(user: ?*const anyopaque, offset: u64, len: u64) callconv(.c) i32 {
    const l: *Log = @ptrCast(@alignCast(@constCast(user orelse return -EINVAL)));
    return l.before(offset, len);
}

/// Reads sector-sized pieces of a checkpoint: the base disk, with deltas 1..k laid over it.
pub fn readCheckpoint(gpa: std.mem.Allocator, base_fd: fd_t, deltas: []const fd_t, size: u64) ![]u8 {
    const img = try gpa.alloc(u8, size);
    errdefer gpa.free(img);
    if (try sys.pread(base_fd, img, 0) != size) return error.ShortRead;
    const words = (size / BS + 63) / 64;
    const m = try gpa.alloc(u64, words);
    defer gpa.free(m);
    for (deltas) |d| {
        if (try sys.pread(d, std.mem.sliceAsBytes(m), size) != words * 8) return error.ShortRead;
        for (m, 0..) |w, wi| {
            var x = w;
            while (x != 0) {
                const b = wi * 64 + @ctz(x);
                x &= x - 1;
                if (try sys.pread(d, img[b * BS ..][0..BS], b * BS) != BS) return error.ShortRead;
            }
        }
    }
    return img;
}

// --- tests ----------------------------------------------------------------------------------

const testing = std.testing;

const Dir = struct {
    path: [64]u8 = undefined,
    len: usize = 0,
    fn make(tag: []const u8) !Dir {
        var d: Dir = .{};
        const s = try std.fmt.bufPrint(&d.path, "/tmp/silo-writelog-{s}-{d}", .{ tag, std.os.linux.getpid() });
        d.len = s.len;
        try sys.mkdir(s);
        return d;
    }
    fn file(d: *const Dir, name: []const u8, size: u64) !fd_t {
        var buf: [128]u8 = undefined;
        const p = try std.fmt.bufPrint(&buf, "{s}/{s}", .{ d.path[0..d.len], name });
        const fd = try sys.open(p, .{ .ACCMODE = .RDWR, .CREAT = true, .TRUNC = true }, 0o644);
        try sys.ftruncate(fd, size);
        return fd;
    }
    fn readOnly(d: *const Dir, name: []const u8) !fd_t {
        var buf: [128]u8 = undefined;
        const p = try std.fmt.bufPrint(&buf, "{s}/{s}", .{ d.path[0..d.len], name });
        return sys.open(p, .{ .ACCMODE = .RDONLY }, 0);
    }
};

fn marked(m: []const u64) u64 {
    var n: u64 = 0;
    for (m) |w| n += @popCount(w);
    return n;
}

test "a range marks every block it touches, and nothing outside the disk" {
    const d = try Dir.make("range");
    const live = try d.file("live", 64 * BS);
    defer sys.close(live);
    const l = try Log.init(testing.allocator, live, 64 * BS);
    defer l.deinit(testing.allocator);
    const h: Hook = Log.hook; // called through libkrun's type
    try testing.expectEqual(@as(i32, 0), h(l.user(), 512, 512)); // inside block 0
    try testing.expectEqual(@as(i32, 0), h(l.user(), 4096 - 512, 1024)); // blocks 0 and 1
    try testing.expectEqual(@as(i32, 0), h(l.user(), 10 * BS, 3 * BS)); // 10, 11, 12
    try testing.expectEqual(@as(i32, 0), h(l.user(), 20 * BS, 0)); // nothing
    try testing.expectEqual(@as(i32, 0), h(l.user(), 63 * BS, BS)); // the last block
    try testing.expectEqual(-EINVAL, h(l.user(), 63 * BS, BS + 512));
    try testing.expectEqual(-EINVAL, h(l.user(), std.math.maxInt(u64) - 511, 1024));
    try testing.expectEqual(-EINVAL, h(null, 0, 512));
    try testing.expectEqual(@as(u64, 0b1_1100_0000_0011) | (@as(u64, 1) << 63), l.cur[0]);
    try testing.expectEqual(@as(u64, 6), marked(l.cur));
}

test "a copy that fails fails the checkpoint, never the write" {
    const d = try Dir.make("fail");
    const live = try d.file("live", 16 * BS);
    defer sys.close(live);
    const ro = d.file("delta", 16 * BS) catch unreachable;
    sys.close(ro);
    const delta = try d.readOnly("delta"); // every pwrite into it fails
    defer sys.close(delta);
    const l = try Log.init(testing.allocator, live, 16 * BS);
    defer l.deinit(testing.allocator);
    try testing.expectEqual(@as(i32, 0), l.before(0, 2 * BS));
    l.begin(delta);
    l.swap();
    try testing.expectEqual(@as(i32, 0), l.before(0, BS)); // copies first, and the copy fails
    try testing.expectError(error.CopyFailed, l.drain());
    try testing.expectEqual(@as(i32, 0), l.before(5 * BS, BS));
}

/// One step of a scripted night: a write's hook, then later its landing.
fn land(live: fd_t, b: u64, v: u8) !void {
    var buf: [BS]u8 = undefined;
    @memset(&buf, v);
    _ = try sys.pwrite(live, &buf, b * BS);
}

fn checkpointOnce(l: *Log, d: *const Dir, name: []const u8, size: u64) !fd_t {
    const fd = try d.file(name, size);
    l.begin(fd);
    l.swap();
    _ = try l.drain();
    return fd;
}

fn writeInFlightCase(remark: bool) !u8 {
    const d = try Dir.make(if (remark) "inflight" else "inflight-bug");
    const size = 8 * BS;
    const live = try d.file("live", size);
    defer sys.close(live);
    try land(live, 3, 0xAA);
    const base = try d.file("base", size);
    defer sys.close(base);
    var all: [8 * BS]u8 = undefined;
    _ = try sys.pread(live, &all, 0);
    _ = try sys.pwrite(base, &all, 0);
    const l = try Log.init(testing.allocator, live, size);
    defer l.deinit(testing.allocator);
    l.remark_in_flight = remark;

    // Block 3 is written: the hook runs, then the checkpoint's moment and its copy come before the
    // write lands. Checkpoint 1 may hold 0xAA or 0xBB for block 3: the write was in flight.
    try testing.expectEqual(@as(i32, 0), l.before(3 * BS, BS));
    const d1 = try checkpointOnce(l, &d, "d1", size);
    defer sys.close(d1);
    try land(live, 3, 0xBB); // the write lands, and libkrun tells main it is done

    // Checkpoint 2, much later, with nothing written in between: it must hold 0xBB.
    const d2 = try checkpointOnce(l, &d, "d2", size);
    defer sys.close(d2);
    const img = try readCheckpoint(testing.allocator, base, &.{ d1, d2 }, size);
    defer testing.allocator.free(img);
    return img[3 * BS];
}

test "a write in flight at a swap is in the next checkpoint" {
    try testing.expectEqual(@as(u8, 0xBB), try writeInFlightCase(true));
}

test "without the in-flight rule, the next checkpoint misses a completed write" {
    try testing.expectEqual(@as(u8, 0xAA), try writeInFlightCase(false));
}

// The night simulated: a thread stands for libkrun's block worker. For each request it calls the
// hook, waits a little (the request in flight), lands it with pwrite, then counts it done. The main
// thread takes checkpoints at random moments. Every sector's content names the request that wrote
// it last. Each checkpoint, read back through the deltas, must hold for every sector what the
// requests done at its moment wrote, or, for the one request that may have been in flight, either.

const SECTOR: u64 = 512;
const Req = struct { first: u64, n: u64, zero: bool };

fn sectorValue(seq: u64, s: u64) u64 {
    return (seq << 32) | s | (1 << 63);
}

const Worker = struct {
    live: fd_t,
    log: *Log,
    reqs: []Req,
    done: std.atomic.Value(u64) = .init(0),
    seed: u64,

    fn run(w: *Worker) void {
        var rng = std.Random.DefaultPrng.init(w.seed);
        const r = rng.random();
        var buf: [64 * SECTOR]u8 align(8) = undefined;
        for (w.reqs, 1..) |q, seq| {
            const off = q.first * SECTOR;
            const len = q.n * SECTOR;
            std.debug.assert(w.log.before(off, len) == 0);
            // Some requests stay in flight long enough for a whole checkpoint to pass.
            switch (r.uintLessThan(u8, 20)) {
                0 => {
                    const ts: linux.timespec = .{ .sec = 0, .nsec = @intCast(r.uintLessThan(u32, 300_000)) };
                    _ = linux.nanosleep(&ts, null);
                },
                1...5 => std.Thread.yield() catch {},
                else => {},
            }
            const words = std.mem.bytesAsSlice(u64, buf[0..len]);
            for (words, 0..) |*x, i| x.* = if (q.zero) 0 else sectorValue(seq, q.first + i / (SECTOR / 8));
            _ = sys.pwrite(w.live, buf[0..len], off) catch unreachable;
            w.done.store(seq, .release);
        }
    }
};

test "every checkpoint holds the disk at its moment, while the computer writes" {
    const gpa = testing.allocator;
    const d = try Dir.make("night");
    const size: u64 = 256 * BS;
    const sectors = size / SECTOR;
    const live = try d.file("live", size);
    defer sys.close(live);
    const base = try d.file("base", size);
    defer sys.close(base);
    {
        const init_img = try gpa.alloc(u8, size);
        defer gpa.free(init_img);
        for (std.mem.bytesAsSlice(u64, init_img), 0..) |*x, i| x.* = sectorValue(0, i / (SECTOR / 8));
        _ = try sys.pwrite(live, init_img, 0);
        _ = try sys.pwrite(base, init_img, 0);
    }

    var rng = std.Random.DefaultPrng.init(0x5110);
    const r = rng.random();
    const nreq = 20000;
    const reqs = try gpa.alloc(Req, nreq);
    defer gpa.free(reqs);
    for (reqs) |*q| {
        const n = 1 + r.uintLessThan(u64, if (r.boolean()) 8 else 64);
        // Writes cluster on a few hot blocks, as a journal's do, so swaps often meet them.
        const first = if (r.boolean()) r.uintLessThan(u64, 16) else r.uintLessThan(u64, sectors - n + 1);
        q.* = .{ .first = first, .n = n, .zero = r.uintLessThan(u8, 10) == 0 };
    }

    const l = try Log.init(gpa, live, size);
    defer l.deinit(gpa);
    var w: Worker = .{ .live = live, .log = l, .reqs = reqs, .seed = 7 };
    const t = try std.Thread.spawn(.{}, Worker.run, .{&w});

    var deltas: std.ArrayList(fd_t) = .empty;
    defer {
        for (deltas.items) |fd| sys.close(fd);
        deltas.deinit(gpa);
    }
    var moments: std.ArrayList(u64) = .empty;
    defer moments.deinit(gpa);
    var name_buf: [16]u8 = undefined;
    while (w.done.load(.acquire) < nreq) {
        var spin = r.uintLessThan(u32, 20000);
        while (spin > 0) : (spin -= 1) std.atomic.spinLoopHint();
        const fd = try d.file(try std.fmt.bufPrint(&name_buf, "d{d}", .{deltas.items.len}), size);
        l.begin(fd);
        l.mu.lock();
        const at = w.done.load(.acquire);
        l.swapLocked();
        l.mu.unlock();
        _ = try l.drain();
        try deltas.append(gpa, fd);
        try moments.append(gpa, at);
    }
    t.join();
    try testing.expect(deltas.items.len >= 10);

    // The expected disk after each request, replayed in order, checked at each moment.
    const want = try gpa.alloc(u64, sectors);
    defer gpa.free(want);
    for (want, 0..) |*x, s| x.* = sectorValue(0, s);
    var applied: u64 = 0;
    var in_flight_seen: u64 = 0;
    for (moments.items, 0..) |at, k| {
        while (applied < at) : (applied += 1) {
            const q = reqs[applied];
            var s = q.first;
            while (s < q.first + q.n) : (s += 1) want[s] = if (q.zero) 0 else sectorValue(applied + 1, s);
        }
        const img = try readCheckpoint(gpa, base, deltas.items[0 .. k + 1], size);
        defer gpa.free(img);
        const next: ?Req = if (at < nreq) reqs[at] else null;
        for (want, 0..) |x, s| {
            const got = std.mem.readInt(u64, img[s * SECTOR ..][0..8], .little);
            if (got == x) continue;
            const q = next orelse return error.CheckpointWrong;
            const in_next = s >= q.first and s < q.first + q.n;
            const its = if (q.zero) 0 else sectorValue(at + 1, s);
            if (!in_next or got != its) {
                std.debug.print("checkpoint {d} (moment {d}): sector {d} holds {x}, want {x}\n", .{ k, at, s, got, x });
                return error.CheckpointWrong;
            }
            in_flight_seen += 1;
        }
    }
}
