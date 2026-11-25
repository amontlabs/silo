//! The stand-in for a running computer: one thread writes 4 KiB blocks at random places of the
//! disk image, uncached, and keeps every write's start, wait and block. Each block carries its
//! number and the write's sequence number, so a checkpoint can be checked against the log: it must
//! hold exactly the writes up to one moment.
const std = @import("std");
const posix = std.posix;
const sys = @import("sys.zig");
const nbd = @import("nbd.zig");

pub const BS: u64 = 4096;
pub const MAGIC: u64 = 0x53494c4f424c4b31; // "SILOBLK1"

pub const Write = struct { start: u64, wait: u64, block: u32 };

/// What the virtual machine monitor would do on each write when Silo owns the disk's writes:
/// mark the block in a log of what changed, and copy a block out before it is overwritten.
pub const Hook = struct {
    ctx: *anyopaque,
    before: *const fn (ctx: *anyopaque, block: u32) void,
};

pub const Writer = struct {
    fd: posix.fd_t,
    blocks: u64,
    rate: u64, // writes per second, 0 = as fast as it can
    prng: std.Random.DefaultPrng,
    buf: []align(4096) u8,
    log: std.ArrayList(Write) = .empty,
    gpa: std.mem.Allocator,
    hook: ?Hook = null,
    nbd: ?*nbd.Client = null, // when the disk is served over NBD
    pause_req: std.atomic.Value(u32) = .init(0),
    paused: std.atomic.Value(u32) = .init(0),
    stop: std.atomic.Value(u32) = .init(0),
    pause_ns: u64 = 0,
    log_lock: std.Thread.Mutex = .{},

    pub fn init(gpa: std.mem.Allocator, path: []const u8, blocks: u64, rate: u64, seed: u64) !Writer {
        const buf = try gpa.alignedAlloc(u8, .fromByteUnits(4096), BS);
        @memset(buf, 0xa5);
        var w = Writer{ .fd = try sys.openImage(path, false), .blocks = blocks, .rate = rate, .prng = .init(seed), .buf = buf, .gpa = gpa };
        try w.log.ensureTotalCapacity(gpa, 16 << 20);
        return w;
    }

    pub fn seq(w: *Writer) u64 {
        return w.log.items.len;
    }

    /// One write; returns its wait in ns.
    pub fn writeOne(w: *Writer) !u64 {
        const b: u32 = @intCast(w.prng.random().uintLessThan(u64, w.blocks));
        const s = w.log.items.len + 1;
        std.mem.writeInt(u64, w.buf[0..8], MAGIC, .little);
        std.mem.writeInt(u64, w.buf[8..16], b, .little);
        std.mem.writeInt(u64, w.buf[16..24], s, .little);
        const t0 = sys.now();
        if (w.hook) |h| h.before(h.ctx, b);
        if (w.nbd) |c| try c.write(w.buf, @as(u64, b) * BS) else _ = try posix.pwrite(w.fd, w.buf, @as(u64, b) * BS);
        const t1 = sys.now();
        w.log_lock.lock();
        w.log.appendAssumeCapacity(.{ .start = t0, .wait = t1 - t0, .block = b });
        w.log_lock.unlock();
        return t1 - t0;
    }

    pub fn run(w: *Writer) void {
        const period: u64 = if (w.rate == 0) 0 else std.time.ns_per_s / w.rate;
        var due = sys.now();
        while (w.stop.load(.acquire) == 0) {
            if (w.pause_req.load(.acquire) != 0) {
                const p0 = sys.now();
                w.paused.store(1, .release);
                while (w.pause_req.load(.acquire) != 0) std.atomic.spinLoopHint();
                w.paused.store(0, .release);
                w.pause_ns += sys.now() - p0;
                due = sys.now();
            }
            _ = w.writeOne() catch |e| {
                std.debug.print("writer: {s}\n", .{@errorName(e)});
                return;
            };
            if (period != 0) {
                due += period;
                const t = sys.now();
                if (due > t) sys.sleepNs(due - t) else due = t;
            }
        }
    }

    /// Stops the computer between two writes, as a pause of the virtual machine does.
    pub fn pause(w: *Writer) void {
        w.pause_req.store(1, .release);
        while (w.paused.load(.acquire) == 0) std.atomic.spinLoopHint();
    }
    pub fn unpause(w: *Writer) void {
        w.pause_req.store(0, .release);
    }

    /// The longest write that overlapped [a, b], and how many writes ended inside it.
    pub fn window(w: *Writer, a: u64, b: u64) struct { longest: u64, during: u64 } {
        w.log_lock.lock();
        defer w.log_lock.unlock();
        var longest: u64 = 0;
        var during: u64 = 0;
        var i = w.log.items.len;
        while (i > 0) {
            i -= 1;
            const e = w.log.items[i];
            const end = e.start + e.wait;
            if (end + 30 * std.time.ns_per_s < a) break;
            if (e.start <= b and end >= a) longest = @max(longest, e.wait);
            if (end >= a and end <= b) during += 1;
        }
        return .{ .longest = longest, .during = during };
    }

    /// Percentiles of the waits of the writes from index `from` on.
    pub fn percentiles(w: *Writer, from: usize) struct { p50: u64, p99: u64, max: u64 } {
        w.log_lock.lock();
        const n = w.log.items.len - from;
        const v = w.gpa.alloc(u64, n) catch unreachable;
        for (w.log.items[from..], 0..) |e, i| v[i] = e.wait;
        w.log_lock.unlock();
        defer w.gpa.free(v);
        if (n == 0) return .{ .p50 = 0, .p99 = 0, .max = 0 };
        std.mem.sort(u64, v, {}, std.sort.asc(u64));
        return .{ .p50 = v[n / 2], .p99 = v[(n * 99) / 100], .max = v[n - 1] };
    }
};

/// Fills the image with every block numbered, sequence 0, in 1 MiB uncached writes.
pub fn fill(gpa: std.mem.Allocator, path: []const u8, blocks: u64) !void {
    const fd = try sys.openImage(path, true);
    defer posix.close(fd);
    const per = 256;
    const buf = try gpa.alignedAlloc(u8, .fromByteUnits(4096), per * BS);
    defer gpa.free(buf);
    @memset(buf, 0x5a);
    var b: u64 = 0;
    while (b < blocks) : (b += per) {
        var i: u64 = 0;
        while (i < per) : (i += 1) {
            const o = i * BS;
            std.mem.writeInt(u64, buf[o..][0..8], MAGIC, .little);
            std.mem.writeInt(u64, buf[o + 8 ..][0..8], b + i, .little);
            std.mem.writeInt(u64, buf[o + 16 ..][0..8], 0, .little);
        }
        _ = try posix.pwrite(fd, buf, b * BS);
    }
    try sys.fullSync(fd);
}

/// Checks that an image holds exactly the writes of the log up to one moment: the highest
/// sequence it holds, S, and for every block the last write to it with a sequence up to S.
/// Returns S, or an error naming the first block that breaks it.
pub fn verify(gpa: std.mem.Allocator, path: []const u8, blocks: u64, log: []const Write) !u64 {
    const fd = try sys.openRead(path);
    defer posix.close(fd);
    const got = try gpa.alloc(u64, blocks);
    defer gpa.free(got);
    const per = 1024;
    const buf = try gpa.alloc(u8, per * BS);
    defer gpa.free(buf);
    var b: u64 = 0;
    var s_max: u64 = 0;
    while (b < blocks) : (b += per) {
        const n = try posix.pread(fd, buf, b * BS);
        var i: u64 = 0;
        while (i * BS < n) : (i += 1) {
            const o = i * BS;
            if (std.mem.readInt(u64, buf[o..][0..8], .little) != MAGIC) return error.NotABlock;
            if (std.mem.readInt(u64, buf[o + 8 ..][0..8], .little) != b + i) return error.WrongBlock;
            const s = std.mem.readInt(u64, buf[o + 16 ..][0..8], .little);
            got[b + i] = s;
            s_max = @max(s_max, s);
        }
    }
    const want = try gpa.alloc(u64, blocks);
    defer gpa.free(want);
    @memset(want, 0);
    for (log[0..s_max], 0..) |e, i| want[e.block] = i + 1;
    for (got, want, 0..) |g, x, i| {
        if (g != x) {
            std.debug.print("verify {s}: block {d} holds write {d}, the moment {d} needs write {d}\n", .{ path, i, g, s_max, x });
            return error.NotAMoment;
        }
    }
    return s_max;
}
