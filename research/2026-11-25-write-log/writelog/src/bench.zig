//! What the hook adds to each write of main, measured on the block worker's side.
//!
//!   idle     no checkpoint being made: the hook marks the block
//!   marking  a checkpoint being copied, the block not in it: the hook marks the block
//!   copying  a checkpoint being copied, the block in it and not copied yet: the hook copies the
//!            4 KiB block from the live disk into the checkpoint first (a pread and a pwrite)
//!
//! The disk is a sparse file of 32 GiB, main's size, so the bitmap is 1 MiB, as on a Mac.
const std = @import("std");
const wl = @import("writelog");
const linux = std.os.linux;

fn now() u64 {
    var ts: linux.timespec = undefined;
    _ = linux.clock_gettime(.MONOTONIC, &ts);
    return @as(u64, @intCast(ts.sec)) * 1_000_000_000 + @as(u64, @intCast(ts.nsec));
}

fn report(name: []const u8, ns: []u64) void {
    std.mem.sort(u64, ns, {}, std.sort.asc(u64));
    std.debug.print("{s:<8} calls {d:>7}  median {d:>6} ns  p99 {d:>7} ns  max {d:>8} ns\n", .{
        name, ns.len, ns[ns.len / 2], ns[ns.len * 99 / 100], ns[ns.len - 1],
    });
}

pub fn main() !void {
    const gpa = std.heap.page_allocator;
    const size: u64 = 32 << 30;
    const blocks = size / wl.BS;
    _ = linux.mkdir("/tmp/silo-writelog-bench", 0o755);
    const live = try wl.sys.open("/tmp/silo-writelog-bench/live", .{ .ACCMODE = .RDWR, .CREAT = true, .TRUNC = true }, 0o644);
    try wl.sys.ftruncate(live, size);
    const delta = try wl.sys.open("/tmp/silo-writelog-bench/delta", .{ .ACCMODE = .RDWR, .CREAT = true, .TRUNC = true }, 0o644);
    try wl.sys.ftruncate(delta, size);
    const l = try wl.Log.init(gpa, live, size);
    var rng = std.Random.DefaultPrng.init(1);
    const r = rng.random();
    const n = 200_000;
    const ns = try gpa.alloc(u64, n);
    const hook = wl.Log.hook;

    // A working set of 1 GiB: what a night writes between two checkpoints.
    const hot = (1 << 30) / wl.BS;
    for (ns) |*t| {
        const b = r.uintLessThan(u64, hot);
        const t0 = now();
        _ = hook(l.user(), b * wl.BS, wl.BS);
        t.* = now() - t0;
    }
    report("idle", ns);

    // Every block of the working set is now in the checkpoint about to be made.
    l.begin(delta);
    l.swap();
    for (ns) |*t| {
        const b = hot + r.uintLessThan(u64, blocks - hot);
        const t0 = now();
        _ = hook(l.user(), b * wl.BS, wl.BS);
        t.* = now() - t0;
    }
    report("marking", ns);

    var k: usize = 0;
    var b: u64 = 0;
    while (k < n) : (b += 1) {
        // Each block once: the first write to a frozen block copies it, later ones do not.
        const t0 = now();
        _ = hook(l.user(), b * wl.BS, wl.BS);
        ns[k] = now() - t0;
        k += 1;
    }
    report("copying", ns);
    _ = try l.drain();
}
