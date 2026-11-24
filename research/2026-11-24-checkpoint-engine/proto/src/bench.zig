//! `ckpt bench`: a computer stand-in writes its disk while checkpoints are taken one way, then
//! checkpoints are checked, compared by block map, and dropped. One JSON object per line.
const std = @import("std");
const posix = std.posix;
const sys = @import("sys.zig");
const W = @import("writer.zig");
const S = @import("strategy.zig");
const nbd = @import("nbd.zig");
const BS = W.BS;

pub const Opts = struct {
    strategy: S.Kind = .full,
    dir: []const u8 = "/mnt/x/run",
    size_gib: u64 = 8,
    frag: u64 = 300_000,
    rate: u64 = 1000,
    interval_ms: u64 = 5000,
    count: u64 = 10,
    verify: bool = true,
    drop: u64 = 3,
    seed: u64 = 1,
    unrelated: bool = false, // also: clone a fragmented file nobody writes, and see if live waits
};

var out_buf: [1 << 16]u8 = undefined;
var out_w: std.fs.File.Writer = undefined;
fn out() *std.Io.Writer {
    return &out_w.interface;
}

fn p(gpa: std.mem.Allocator, comptime fmt: []const u8, args: anytype) ![:0]u8 {
    return std.fmt.allocPrintSentinel(gpa, fmt, args, 0);
}

fn pieces(gpa: std.mem.Allocator, path: []const u8, size: u64) !u64 {
    if (!sys.is_linux) return 0;
    var l = try S.mapOf(gpa, path, size);
    defer l.deinit(gpa);
    return l.items.len;
}

fn removeCkpt(gpa: std.mem.Allocator, o: Opts, i: u64) !void {
    if (o.strategy == .btrfs) {
        const snaps = try p(gpa, "{s}/snaps", .{o.dir});
        const name = try p(gpa, "ckpt-{d}", .{i});
        try sys.btrfsDestroy(snaps, name);
    } else if (o.strategy == .delta) {
        _ = try S.mergeInto(gpa, try p(gpa, "{s}/ckpt-{d}.img", .{ o.dir, i }), try p(gpa, "{s}/ckpt-{d}.img", .{ o.dir, i + 1 }), o.size_gib << 30);
    } else {
        const path = try p(gpa, "{s}/ckpt-{d}.img", .{ o.dir, i });
        try posix.unlink(path);
    }
}

/// The path to read checkpoint i as a whole disk; for deltas, a materialized copy.
fn readable(gpa: std.mem.Allocator, o: Opts, i: u64) ![:0]u8 {
    if (o.strategy != .delta or i == 0) return ckptPath(gpa, o, i);
    var ds: std.ArrayList([:0]const u8) = .empty;
    var k: u64 = 1;
    while (k <= i) : (k += 1) {
        const d = try ckptPath(gpa, o, k);
        std.fs.cwd().access(d, .{}) catch continue;
        try ds.append(gpa, d);
    }
    const out_path = try p(gpa, "{s}/read-{d}.img", .{ o.dir, i });
    std.fs.cwd().deleteFile(out_path) catch {};
    try S.materialize(gpa, try ckptPath(gpa, o, 0), ds.items, out_path, o.size_gib << 30);
    return out_path;
}

fn ckptPath(gpa: std.mem.Allocator, o: Opts, i: u64) ![:0]u8 {
    return if (o.strategy == .btrfs) p(gpa, "{s}/snaps/ckpt-{d}/live.img", .{ o.dir, i }) else p(gpa, "{s}/ckpt-{d}.img", .{ o.dir, i });
}

pub fn run(gpa: std.mem.Allocator, o: Opts) !void {
    out_w = std.fs.File.stdout().writer(&out_buf);
    const size = o.size_gib << 30;
    const blocks = size / BS;
    std.fs.cwd().makePath(o.dir) catch {};
    var live: [:0]u8 = undefined;
    if (o.strategy == .btrfs) {
        try sys.btrfsCreateSubvol(o.dir, "vol");
        const snaps = try p(gpa, "{s}/snaps", .{o.dir});
        try posix.mkdir(snaps, 0o755);
        live = try p(gpa, "{s}/vol/live.img", .{o.dir});
    } else live = try p(gpa, "{s}/live.img", .{o.dir});

    const tf = sys.now();
    try W.fill(gpa, live, blocks);
    var w = try W.Writer.init(gpa, live, blocks, o.rate, o.seed);
    try out().print("{{\"phase\":\"fill\",\"strategy\":\"{s}\",\"gib\":{d},\"s\":{d:.2}}}\n", .{ @tagName(o.strategy), o.size_gib, sys.secs(sys.now() - tf) });
    try out().flush();

    // Pieces, as weeks of checkpoints leave them: share every block once, then scatter writes.
    if (o.frag > 0) {
        const t0 = sys.now();
        if (o.strategy == .btrfs) {
            try sys.btrfsSnapshot(try p(gpa, "{s}/vol", .{o.dir}), try p(gpa, "{s}/snaps", .{o.dir}), "anchor", true);
        } else try sys.cloneFile(live, try p(gpa, "{s}/anchor.img", .{o.dir}));
        var k: u64 = 0;
        while (k < o.frag) : (k += 1) _ = try w.writeOne();
        if (o.strategy == .btrfs) try sys.btrfsDestroy(try p(gpa, "{s}/snaps", .{o.dir}), "anchor") else try posix.unlink(try p(gpa, "{s}/anchor.img", .{o.dir}));
        try out().print("{{\"phase\":\"frag\",\"writes\":{d},\"pieces\":{d},\"s\":{d:.1}}}\n", .{ o.frag, try pieces(gpa, live, size), sys.secs(sys.now() - t0) });
        try out().flush();
    }

    // The checkpoint at start, with nothing running.
    var seqs: std.ArrayList(u64) = .empty;
    {
        const t0 = sys.now();
        if (o.strategy == .btrfs) {
            _ = try S.btrfs(try p(gpa, "{s}/vol", .{o.dir}), try p(gpa, "{s}/snaps", .{o.dir}), "ckpt-0");
        } else {
            _ = try S.full(live, try p(gpa, "{s}/tmp-0.img", .{o.dir}), try ckptPath(gpa, o, 0), o.dir);
        }
        try seqs.append(gpa, w.seq());
        try out().print("{{\"phase\":\"base\",\"s\":{d:.3},\"pieces\":{d}}}\n", .{ sys.secs(sys.now() - t0), try pieces(gpa, live, size) });
        try out().flush();
    }

    var cbw: ?*S.Cbw = null;
    if (o.strategy == .cbw or o.strategy == .delta) {
        cbw = try S.Cbw.init(gpa, live, blocks);
        if (o.strategy == .delta) {
            cbw.?.delta = true;
            cbw.?.size = size;
            cbw.?.map = try gpa.alloc(u64, cbw.?.cur.len);
        }
        w.hook = cbw.?.hook();
    }

    var server: nbd.Server = undefined;
    var client: nbd.Client = undefined;
    if (o.strategy == .nbd) {
        // The computer's writes reach the image only through Silo's NBD server.
        var sp: [2]posix.fd_t = undefined;
        if (sys.is_linux) {
            if (posix.errno(std.os.linux.socketpair(posix.AF.UNIX, posix.SOCK.STREAM, 0, &sp)) != .SUCCESS) return error.SocketPair;
        } else if (std.c.socketpair(posix.AF.UNIX, posix.SOCK.STREAM, 0, &sp) != 0) return error.SocketPair;
        cbw = try S.Cbw.init(gpa, live, blocks);
        server = try nbd.Server.init(gpa, live, size, cbw);
        _ = try std.Thread.spawn(.{}, nbd.Server.serve, .{ &server, sp[0] });
        client = try nbd.Client.connect(sp[1]);
        w.nbd = &client;
    }

    const first_live = w.seq();
    const th = try std.Thread.spawn(.{}, W.Writer.run, .{&w});
    sys.sleepNs(2 * std.time.ns_per_s);
    const base = w.percentiles(first_live);
    try out().print("{{\"phase\":\"baseline\",\"p50_ms\":{d:.3},\"p99_ms\":{d:.3},\"max_ms\":{d:.3}}}\n", .{ ms(base.p50), ms(base.p99), ms(base.max) });
    try out().flush();

    var i: u64 = 1;
    while (i <= o.count) : (i += 1) {
        sys.sleepNs(o.interval_ms * std.time.ns_per_ms);
        const tmp = try p(gpa, "{s}/tmp-{d}.img", .{ o.dir, i });
        const dst = try ckptPath(gpa, o, if (o.strategy == .none) 0 else i);
        const prev = try ckptPath(gpa, o, i - 1);
        const t0 = sys.now();
        const r = switch (o.strategy) {
            .none => blk: {
                // No checkpoint: the waits a computer sees anyway on this disk, for comparison.
                sys.sleepNs(std.time.ns_per_s);
                break :blk S.Result{ .op_ns = std.time.ns_per_s };
            },
            .full => try S.full(live, tmp, dst, o.dir),
            .range => try S.range(gpa, &w, live, prev, tmp, dst, o.dir, size),
            .cbw, .delta => try cbw.?.checkpoint(&w, prev, tmp, dst, o.dir, blocks),
            .nbd => try server.checkpoint(prev, tmp, dst, o.dir),
            .btrfs => try S.btrfs(try p(gpa, "{s}/vol", .{o.dir}), try p(gpa, "{s}/snaps", .{o.dir}), try p(gpa, "ckpt-{d}", .{i})),
        };
        const t1 = sys.now();
        const win = w.window(t0, t1);
        // The waits of each phase: cloning the last checkpoint, then the rest.
        const wc = if (r.clone_end != 0) w.window(t0, r.clone_end) else win;
        const wr = if (r.clone_end != 0) w.window(r.clone_end + 1, if (r.publish_at != 0) r.publish_at else t1) else win;
        const wp = if (r.publish_at != 0) w.window(r.publish_at, t1) else win;
        try seqs.append(gpa, w.seq());
        try out().print("{{\"phase\":\"ckpt\",\"n\":{d},\"op_s\":{d:.4},\"total_s\":{d:.4},\"pause_ms\":{d:.3},\"longest_write_ms\":{d:.2},\"in_clone_ms\":{d:.2},\"after_clone_ms\":{d:.2},\"publish_ms\":{d:.2},\"writes_during\":{d},\"ranges\":{d},\"copied_mib\":{d:.1},\"cow_copies\":{d}}}\n", .{ i, sys.secs(r.op_ns), sys.secs(t1 - t0), ms(r.pause_ns), ms(win.longest), ms(wc.longest), ms(wr.longest), ms(wp.longest), win.during, r.ranges, @as(f64, @floatFromInt(r.bytes_copied)) / (1 << 20), r.cow_copies });
        try out().flush();
    }
    if (o.unrelated and o.strategy != .none and o.strategy != .delta) {
        // Clone a checkpoint nobody writes (what range and cbw do first), and see whether the live
        // image's writes wait for it.
        const src = try ckptPath(gpa, o, o.count);
        const dst = try p(gpa, "{s}/unrelated.img", .{o.dir});
        const t0 = sys.now();
        try sys.cloneFile(src, dst);
        const t1 = sys.now();
        const win = w.window(t0, t1);
        try out().print("{{\"phase\":\"unrelated-clone\",\"pieces\":{d},\"op_s\":{d:.3},\"longest_write_ms\":{d:.2},\"writes_during\":{d}}}\n", .{ try pieces(gpa, src, size), sys.secs(t1 - t0), ms(win.longest), win.during });
        try posix.unlink(dst);
        try out().flush();
    }
    const all = w.percentiles(first_live);
    try out().print("{{\"phase\":\"run\",\"writes\":{d},\"p50_ms\":{d:.3},\"p99_ms\":{d:.3},\"max_ms\":{d:.2},\"paused_ms\":{d:.3},\"pieces_live\":{d}}}\n", .{ w.seq() - first_live, ms(all.p50), ms(all.p99), ms(all.max), ms(w.pause_ns), try pieces(gpa, live, size) });
    try out().flush();

    // Dropping old checkpoints while the computer writes (thinning).
    i = 1;
    while (i <= o.drop and i < o.count) : (i += 1) {
        const t0 = sys.now();
        try removeCkpt(gpa, o, i);
        const t1 = sys.now();
        sys.sleepNs(3 * std.time.ns_per_s); // what the file system frees after the call returns
        const win = w.window(t0, sys.now());
        try out().print("{{\"phase\":\"drop\",\"n\":{d},\"op_s\":{d:.4},\"longest_write_ms_3s\":{d:.2}}}\n", .{ i, sys.secs(t1 - t0), ms(win.longest) });
        try out().flush();
    }
    w.stop.store(1, .release);
    th.join();

    if (o.verify) {
        // Every kept checkpoint holds exactly the writes up to one moment, in order.
        var last: u64 = 0;
        var k: u64 = 0;
        while (k <= o.count) : (k += 1) {
            if (k >= 1 and k <= o.drop and k < o.count) continue;
            const t0 = sys.now();
            const path = try readable(gpa, o, k);
            const s = W.verify(gpa, path, blocks, w.log.items) catch |e| {
                try out().print("{{\"phase\":\"verify\",\"n\":{d},\"ok\":false,\"error\":\"{s}\"}}\n", .{ k, @errorName(e) });
                try out().flush();
                continue;
            };
            const ordered = s >= last;
            last = s;
            try out().print("{{\"phase\":\"verify\",\"n\":{d},\"ok\":{},\"moment\":{d},\"s\":{d:.1}}}\n", .{ k, ordered, s, sys.secs(sys.now() - t0) });
            if (o.strategy == .delta and k > 0) try posix.unlink(path);
            try out().flush();
        }
    }

    if (sys.is_linux and o.strategy != .delta) {
        // What changed between the last two checkpoints, from their block maps only, against the
        // writes the log says landed between them.
        const a = try ckptPath(gpa, o, o.count - 1);
        const b = try ckptPath(gpa, o, o.count);
        const sa = W.verify(gpa, a, blocks, w.log.items) catch 0;
        const sb = W.verify(gpa, b, blocks, w.log.items) catch 0;
        const t0 = sys.now();
        var ma = try S.mapOf(gpa, a, size);
        var mb = try S.mapOf(gpa, b, size);
        var d: std.ArrayList([2]u64) = .empty;
        try S.diffMaps(gpa, ma.items, mb.items, size, &d);
        const t1 = sys.now();
        const marked = try gpa.alloc(bool, blocks);
        @memset(marked, false);
        var diff_blocks: u64 = 0;
        for (d.items) |x| {
            var bb = x[0] / BS;
            while (bb < x[1] / BS) : (bb += 1) marked[bb] = true;
            diff_blocks += (x[1] - x[0]) / BS;
        }
        const truth = try gpa.alloc(bool, blocks);
        @memset(truth, false);
        var true_blocks: u64 = 0;
        var missed: u64 = 0;
        if (sb > sa) for (w.log.items[sa..sb]) |e| {
            if (!truth[e.block]) true_blocks += 1;
            truth[e.block] = true;
        };
        for (truth, marked) |t, m| if (t and !m) {
            missed += 1;
        };
        try out().print("{{\"phase\":\"diff\",\"extents_a\":{d},\"extents_b\":{d},\"map_s\":{d:.3},\"ranges\":{d},\"blocks_in_diff\":{d},\"blocks_written\":{d},\"missed\":{d}}}\n", .{ ma.items.len, mb.items.len, sys.secs(t1 - t0), d.items.len, diff_blocks, true_blocks, missed });
        try out().flush();
        ma.deinit(gpa);
        mb.deinit(gpa);
    }
}

fn ms(ns: u64) f64 {
    return @as(f64, @floatFromInt(ns)) / 1e6;
}
