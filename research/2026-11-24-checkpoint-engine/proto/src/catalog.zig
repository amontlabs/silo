//! The checkpoint catalog with no database: the directory is the catalog.
//!
//!   <root>/live/            the computer: disk.img and aux.bin (a macOS computer's auxiliary
//!                           storage, which must always go with its disk, #352 file 07)
//!   <root>/ckpt/<id>/       one complete checkpoint: disk.img, aux.bin, meta
//!   <root>/ckpt/.tmp-<id>/  a checkpoint being made; never read, removed at start
//!   <root>/ckpt/.trash-<id>/  a checkpoint being removed; removed at start
//!   <root>/live.new/        a restore being prepared; removed at start
//!   <root>/next-id          the next id, so an id is never given twice
//!
//! A checkpoint appears by one rename and disappears by one rename, so it is whole or absent. A
//! restore swaps the computer's whole folder in one call (renameat2 RENAME_EXCHANGE on Linux,
//! renamex_np RENAME_SWAP on macOS), so the disk and its auxiliary storage never come from two
//! moments.
//!
//! `ckpt crash` runs these operations in a child process, kills it with SIGKILL at a random moment,
//! recovers, and checks what must hold. It tests a process crash, not a power cut: what a power
//! cut keeps depends on the flushes, which need a Mac (F_FULLFSYNC) or a device that drops writes.
const std = @import("std");
const posix = std.posix;
const sys = @import("sys.zig");

const DISK = 64 * 1024;

fn path(buf: []u8, comptime fmt: []const u8, args: anytype) [:0]const u8 {
    return std.fmt.bufPrintZ(buf, fmt, args) catch unreachable;
}

fn writeFile(p: [:0]const u8, data: []const u8) !void {
    const fd = try posix.open(p, .{ .ACCMODE = .WRONLY, .CREAT = true, .TRUNC = true }, 0o644);
    defer posix.close(fd);
    _ = try posix.pwrite(fd, data, 0);
    try sys.fullSync(fd);
}

fn readU64(p: [:0]const u8, off: u64) !u64 {
    const fd = try posix.open(p, .{ .ACCMODE = .RDONLY }, 0);
    defer posix.close(fd);
    var b: [8]u8 = undefined;
    if (try posix.pread(fd, &b, off) != 8) return error.Short;
    return std.mem.readInt(u64, &b, .little);
}

/// Set CKPT_MUTANT to break one rule on purpose and see the crash test catch it:
/// "two-renames" swaps the computer's folder with two renames instead of one exchange;
/// "no-tmp" builds a checkpoint in its final place instead of a temporary one.
fn mutant(name: []const u8) bool {
    const v = posix.getenv("CKPT_MUTANT") orelse return false;
    return std.mem.eql(u8, v, name);
}

/// Set CKPT_CRASH_AT=k to have the process kill itself at the k-th step of the catalog's work:
/// the systematic crash test tries every step in turn.
var crash_at: ?u64 = null;
var steps: u64 = 0;
fn crashPoint() void {
    steps += 1;
    if (crash_at) |k| if (steps == k) posix.raise(posix.SIG.KILL) catch {};
}

/// Atomic exchange of two directories.
fn exchange(a: [:0]const u8, b: [:0]const u8) !void {
    if (mutant("two-renames")) {
        var buf: [512]u8 = undefined;
        const old = std.fmt.bufPrintZ(&buf, "{s}.old", .{b}) catch unreachable;
        try posix.rename(b, old);
        crashPoint();
        try posix.rename(a, b);
        try posix.rename(old, a);
        return;
    }
    if (sys.is_linux) {
        const rc = std.os.linux.renameat2(posix.AT.FDCWD, a, posix.AT.FDCWD, b, 2); // RENAME_EXCHANGE
        if (posix.errno(rc) != .SUCCESS) return error.ExchangeFailed;
    } else {
        const renamex_np = struct {
            extern "c" fn renamex_np(from: [*:0]const u8, to: [*:0]const u8, flags: c_uint) c_int;
        }.renamex_np;
        if (renamex_np(a.ptr, b.ptr, 0x2) != 0) return error.ExchangeFailed; // RENAME_SWAP
    }
}

fn rmTree(p: []const u8) void {
    std.fs.cwd().deleteTree(p) catch {};
}

pub const Catalog = struct {
    root: []const u8,

    /// Puts the folder back in a state where every rule holds, after a crash at any point.
    pub fn recover(c: Catalog) !void {
        var b: [512]u8 = undefined;
        rmTree(path(&b, "{s}/live.new", .{c.root}));
        crashPoint();
        var d = try std.fs.cwd().openDir(path(&b, "{s}/ckpt", .{c.root}), .{ .iterate = true });
        defer d.close();
        var it = d.iterate();
        var doomed: std.ArrayList([64]u8) = .empty;
        defer doomed.deinit(std.heap.page_allocator);
        while (try it.next()) |e| {
            if (e.name[0] == '.') {
                var n: [64]u8 = @splat(0);
                @memcpy(n[0..e.name.len], e.name);
                try doomed.append(std.heap.page_allocator, n);
            }
        }
        for (doomed.items) |n| d.deleteTree(std.mem.sliceTo(&n, 0)) catch {};
        try sys.syncDir(path(&b, "{s}/ckpt", .{c.root}));
        crashPoint();
    }

    fn nextId(c: Catalog) !u64 {
        var b: [512]u8 = undefined;
        const p = path(&b, "{s}/next-id", .{c.root});
        const id = readU64(p, 0) catch 1;
        // Written before the id is used: after a crash the id is skipped, never given twice.
        var b2: [512]u8 = undefined;
        const tmp = path(&b2, "{s}/next-id.tmp", .{c.root});
        var v: [8]u8 = undefined;
        std.mem.writeInt(u64, &v, id + 1, .little);
        try writeFile(tmp, &v);
        crashPoint();
        try posix.rename(tmp, p);
        crashPoint();
        try sys.syncDir(c.root);
        crashPoint();
        return id;
    }

    /// Takes a checkpoint of the live computer; returns its id.
    pub fn take(c: Catalog, kind: u8) !u64 {
        const id = try c.nextId();
        var b: [512]u8 = undefined;
        var b2: [512]u8 = undefined;
        if (mutant("no-tmp")) {
            const dir = path(&b, "{s}/ckpt/{d}", .{ c.root, id });
            try posix.mkdir(dir, 0o755);
            crashPoint();
            try sys.cloneFile(path(&b2, "{s}/live/disk.img", .{c.root}), path(&b, "{s}/ckpt/{d}/disk.img", .{ c.root, id }));
            crashPoint();
            try sys.cloneFile(path(&b2, "{s}/live/aux.bin", .{c.root}), path(&b, "{s}/ckpt/{d}/aux.bin", .{ c.root, id }));
            crashPoint();
            var m: [16]u8 = undefined;
            std.mem.writeInt(u64, m[0..8], id, .little);
            std.mem.writeInt(u64, m[8..16], kind, .little);
            try writeFile(path(&b, "{s}/ckpt/{d}/meta", .{ c.root, id }), &m);
            crashPoint();
            return id;
        }
        const tmp = path(&b, "{s}/ckpt/.tmp-{d}", .{ c.root, id });
        try posix.mkdir(tmp, 0o755);
        crashPoint();
        try sys.cloneFile(path(&b2, "{s}/live/disk.img", .{c.root}), path(&b, "{s}/ckpt/.tmp-{d}/disk.img", .{ c.root, id }));
        crashPoint();
        try sys.cloneFile(path(&b2, "{s}/live/aux.bin", .{c.root}), path(&b, "{s}/ckpt/.tmp-{d}/aux.bin", .{ c.root, id }));
        crashPoint();
        var meta: [16]u8 = undefined;
        std.mem.writeInt(u64, meta[0..8], id, .little);
        std.mem.writeInt(u64, meta[8..16], kind, .little);
        try writeFile(path(&b, "{s}/ckpt/.tmp-{d}/meta", .{ c.root, id }), &meta);
        crashPoint();
        for ([_][]const u8{ "disk.img", "aux.bin" }) |f| {
            const fd = try posix.open(path(&b, "{s}/ckpt/.tmp-{d}/{s}", .{ c.root, id, f }), .{ .ACCMODE = .RDONLY }, 0);
            defer posix.close(fd);
            try sys.fullSync(fd);
            crashPoint();
        }
        try sys.syncDir(path(&b, "{s}/ckpt/.tmp-{d}", .{ c.root, id }));
        crashPoint();
        try posix.rename(path(&b, "{s}/ckpt/.tmp-{d}", .{ c.root, id }), path(&b2, "{s}/ckpt/{d}", .{ c.root, id }));
        crashPoint();
        try sys.syncDir(path(&b, "{s}/ckpt", .{c.root}));
        crashPoint();
        return id;
    }

    pub fn remove(c: Catalog, id: u64) !void {
        var b: [512]u8 = undefined;
        var b2: [512]u8 = undefined;
        const trash = path(&b2, "{s}/ckpt/.trash-{d}", .{ c.root, id });
        try posix.rename(path(&b, "{s}/ckpt/{d}", .{ c.root, id }), trash);
        crashPoint();
        try sys.syncDir(path(&b, "{s}/ckpt", .{c.root}));
        crashPoint();
        rmTree(trash);
        crashPoint();
    }

    /// Puts the computer back to checkpoint `id`, after saving where it was. The computer is
    /// stopped while this runs.
    pub fn restore(c: Catalog, id: u64) !u64 {
        const saved = try c.take(3);
        var b: [512]u8 = undefined;
        var b2: [512]u8 = undefined;
        const new = path(&b, "{s}/live.new", .{c.root});
        rmTree(new);
        crashPoint();
        try posix.mkdir(new, 0o755);
        crashPoint();
        try sys.cloneFile(path(&b2, "{s}/ckpt/{d}/disk.img", .{ c.root, id }), path(&b, "{s}/live.new/disk.img", .{c.root}));
        crashPoint();
        try sys.cloneFile(path(&b2, "{s}/ckpt/{d}/aux.bin", .{ c.root, id }), path(&b, "{s}/live.new/aux.bin", .{c.root}));
        crashPoint();
        try sys.syncDir(path(&b, "{s}/live.new", .{c.root}));
        crashPoint();
        try exchange(path(&b, "{s}/live.new", .{c.root}), path(&b2, "{s}/live", .{c.root}));
        crashPoint();
        try sys.syncDir(c.root);
        crashPoint();
        rmTree(path(&b, "{s}/live.new", .{c.root}));
        crashPoint();
        return saved;
    }

    /// Silo stops the computer and starts it again: the firmware writes a new boot id to both the
    /// disk and the auxiliary storage. Done on a copy and swapped in, as a restore is.
    pub fn boot(c: Catalog, n: u64) !void {
        var b: [512]u8 = undefined;
        var b2: [512]u8 = undefined;
        rmTree(path(&b, "{s}/live.new", .{c.root}));
        crashPoint();
        try posix.mkdir(path(&b, "{s}/live.new", .{c.root}), 0o755);
        crashPoint();
        try sys.cloneFile(path(&b2, "{s}/live/disk.img", .{c.root}), path(&b, "{s}/live.new/disk.img", .{c.root}));
        crashPoint();
        var v: [8]u8 = undefined;
        std.mem.writeInt(u64, &v, 1000 + n, .little);
        const fd = try posix.open(path(&b, "{s}/live.new/disk.img", .{c.root}), .{ .ACCMODE = .WRONLY }, 0);
        _ = try posix.pwrite(fd, &v, 0);
        crashPoint();
        try sys.fullSync(fd);
        crashPoint();
        posix.close(fd);
        try writeFile(path(&b, "{s}/live.new/aux.bin", .{c.root}), &v);
        crashPoint();
        try sys.syncDir(path(&b, "{s}/live.new", .{c.root}));
        crashPoint();
        try exchange(path(&b, "{s}/live.new", .{c.root}), path(&b2, "{s}/live", .{c.root}));
        crashPoint();
        try sys.syncDir(c.root);
        crashPoint();
        rmTree(path(&b, "{s}/live.new", .{c.root}));
        crashPoint();
    }

    /// The computer writes its disk; its auxiliary storage changes only at a "boot", here every
    /// 16th write, when both files are written in turn (as a firmware update would leave them).
    pub fn computerWrites(c: Catalog, n: u64) !void {
        var b: [512]u8 = undefined;
        var buf: [16]u8 = undefined;
        const disk = path(&b, "{s}/live/disk.img", .{c.root});
        const boot_id = try readU64(disk, 0);
        std.mem.writeInt(u64, buf[0..8], boot_id, .little);
        std.mem.writeInt(u64, buf[8..16], n, .little);
        const fd = try posix.open(disk, .{ .ACCMODE = .WRONLY }, 0);
        defer posix.close(fd);
        _ = try posix.pwrite(fd, &buf, 0);
        crashPoint();
    }
};

fn initRoot(root: []const u8) !void {
    var b: [512]u8 = undefined;
    std.fs.cwd().makePath(path(&b, "{s}/ckpt", .{root})) catch {};
    std.fs.cwd().makePath(path(&b, "{s}/live", .{root})) catch {};
    var data: [DISK]u8 = @splat(0x11);
    std.mem.writeInt(u64, data[0..8], 7, .little); // boot id 7
    try writeFile(path(&b, "{s}/live/disk.img", .{root}), &data);
    try writeFile(path(&b, "{s}/live/aux.bin", .{root}), data[0..8]);
}

/// The child: random operations, each one reported on stdout once it is durable.
pub fn child(root: []const u8, seed: u64) !void {
    if (posix.getenv("CKPT_CRASH_AT")) |v| crash_at = try std.fmt.parseInt(u64, v, 10);
    const c = Catalog{ .root = root };
    var ob: [256]u8 = undefined;
    var ow = std.fs.File.stdout().writer(&ob);
    const out = &ow.interface;
    var prng = std.Random.DefaultPrng.init(seed);
    const r = prng.random();
    var n: u64 = 0;
    while (true) {
        n += 1;
        switch (r.uintLessThan(u8, 11)) {
            0...3 => try c.computerWrites(n),
            4...6 => {
                const id = try c.take(0);
                try out.print("done {d}\n", .{id});
                try out.flush();
            },
            7, 8 => {
                const ids = try list(root);
                if (ids.len > 2) {
                    const id = ids[r.uintLessThan(usize, ids.len - 1)];
                    try c.remove(id);
                    try out.print("removed {d}\n", .{id});
                    try out.flush();
                }
            },
            9 => try c.boot(n),
            else => {
                const ids = try list(root);
                if (ids.len > 0) {
                    const target = ids[r.uintLessThan(usize, ids.len)];
                    try out.print("restoring {d}\n", .{target});
                    try out.flush();
                    const saved = try c.restore(target);
                    try out.print("done {d}\n", .{saved});
                    try out.flush();
                }
            },
        }
    }
}

fn list(root: []const u8) ![]u64 {
    var b: [512]u8 = undefined;
    var d = try std.fs.cwd().openDir(path(&b, "{s}/ckpt", .{root}), .{ .iterate = true });
    defer d.close();
    var it = d.iterate();
    var l: std.ArrayList(u64) = .empty;
    while (try it.next()) |e| {
        if (e.name[0] == '.') continue;
        try l.append(std.heap.page_allocator, try std.fmt.parseInt(u64, e.name, 10));
    }
    std.mem.sort(u64, l.items, {}, std.sort.asc(u64));
    return l.items;
}

/// What must hold after any crash and a recovery.
fn check(root: []const u8, done: *std.AutoHashMap(u64, void)) !void {
    var b: [512]u8 = undefined;
    // The computer is a disk and auxiliary storage of the same boot.
    const disk_boot = try readU64(path(&b, "{s}/live/disk.img", .{root}), 0);
    const aux_boot = try readU64(path(&b, "{s}/live/aux.bin", .{root}), 0);
    if (disk_boot != aux_boot) return error.LivePairMismatch;
    std.fs.cwd().access(path(&b, "{s}/live.new", .{root}), .{}) catch |e| if (e != error.FileNotFound) return e;
    const ids = try list(root);
    var seen = std.AutoHashMap(u64, void).init(std.heap.page_allocator);
    defer seen.deinit();
    for (ids) |id| {
        // Every checkpoint is whole: its three files, of one moment.
        const m = try readU64(path(&b, "{s}/ckpt/{d}/meta", .{ root, id }), 0);
        if (m != id) return error.MetaMismatch;
        const db = try readU64(path(&b, "{s}/ckpt/{d}/disk.img", .{ root, id }), 0);
        const ab = try readU64(path(&b, "{s}/ckpt/{d}/aux.bin", .{ root, id }), 0);
        if (db != ab) return error.CheckpointPairMismatch;
        try seen.put(id, {});
    }
    // Every checkpoint reported done and not reported removed is still there.
    var it = done.keyIterator();
    while (it.next()) |id| if (!seen.contains(id.*)) return error.LostCheckpoint;
    // Nothing half-made is left.
    var d = try std.fs.cwd().openDir(path(&b, "{s}/ckpt", .{root}), .{ .iterate = true });
    defer d.close();
    var di = d.iterate();
    while (try di.next()) |e| if (e.name[0] == '.') return error.LeftOver;
}

pub const CrashOpts = struct {
    root: []const u8 = "/mnt/x/crash",
    rounds: u64 = 500,
    max_ms: u64 = 40,
    seed: u64 = 1,
    /// Kill the child at its k-th step, for every k up to `rounds`, instead of at a random time.
    systematic: bool = false,
};

pub fn crashTest(gpa: std.mem.Allocator, o: CrashOpts) !void {
    try initRoot(o.root);
    var prng = std.Random.DefaultPrng.init(o.seed);
    const r = prng.random();
    var done = std.AutoHashMap(u64, void).init(gpa);
    var ever = std.AutoHashMap(u64, void).init(gpa);
    const self = try std.fs.selfExePathAlloc(gpa);
    var ops: u64 = 0;
    var restores: u64 = 0;
    var round: u64 = 0;
    while (round < o.rounds) : (round += 1) {
        var seed_buf: [32]u8 = undefined;
        const seed_s = try std.fmt.bufPrint(&seed_buf, "{d}", .{o.seed * 100_000 + round});
        var ch = std.process.Child.init(&.{ self, "crash-child", o.root, seed_s }, gpa);
        ch.stdout_behavior = .Pipe;
        var env = try std.process.getEnvMap(gpa);
        defer env.deinit();
        var k_buf: [32]u8 = undefined;
        if (o.systematic) try env.put("CKPT_CRASH_AT", try std.fmt.bufPrint(&k_buf, "{d}", .{round + 1}));
        ch.env_map = &env;
        try ch.spawn();
        if (!o.systematic) {
            sys.sleepNs(r.intRangeAtMost(u64, 1, o.max_ms) * std.time.ns_per_ms);
            posix.kill(ch.id, posix.SIG.KILL) catch {};
        }
        var reader_buf: [4096]u8 = undefined;
        var rd = ch.stdout.?.reader(&reader_buf);
        const all = try rd.interface.allocRemaining(gpa, .unlimited);
        _ = try ch.wait();
        var lines = std.mem.tokenizeScalar(u8, all, '\n');
        while (lines.next()) |l| {
            ops += 1;
            if (std.mem.startsWith(u8, l, "done ")) {
                const id = try std.fmt.parseInt(u64, l[5..], 10);
                if (ever.contains(id)) return error.IdGivenTwice;
                try ever.put(id, {});
                try done.put(id, {});
            } else if (std.mem.startsWith(u8, l, "removed ")) {
                _ = done.remove(try std.fmt.parseInt(u64, l[8..], 10));
            } else if (std.mem.startsWith(u8, l, "restoring ")) restores += 1;
        }
        // A removal cut short may or may not have happened; either is allowed.
        try (Catalog{ .root = o.root }).recover();
        const ids = try list(o.root);
        var it = done.keyIterator();
        var gone: std.ArrayList(u64) = .empty;
        while (it.next()) |id| if (std.mem.indexOfScalar(u64, ids, id.*) == null) try gone.append(gpa, id.*);
        // Only an id the child was removing when killed may be missing; check() refuses others.
        if (gone.items.len > 1) return error.LostCheckpoint;
        for (gone.items) |g| _ = done.remove(g);
        check(o.root, &done) catch |e| {
            std.debug.print("round {d}: {s}\n", .{ round, @errorName(e) });
            return e;
        };
    }
    var ob: [256]u8 = undefined;
    var ow = std.fs.File.stdout().writer(&ob);
    try ow.interface.print("{{\"rounds\":{d},\"ops_reported\":{d},\"restores_started\":{d},\"checkpoints_now\":{d},\"ok\":true}}\n", .{ o.rounds, ops, restores, (try list(o.root)).len });
    try ow.interface.flush();
}
