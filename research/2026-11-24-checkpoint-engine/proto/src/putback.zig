//! `ckpt putback before=DIR after=DIR live=DIR [apply=1]`: put back what one run changed, as
//! `putting-back-a-runs-paths` answers it. `before` and `after` are the computer's files at the
//! checkpoints around the run, `live` its files now. A path the run changed goes back in one step
//! when nobody touched it since (live is as the run left it); otherwise it is only shown.
//!
//! This prototype finds the run's paths by walking both trees. On a real disk the block-map
//! comparison of two checkpoints (bench's "diff") narrows the walk to what changed.
const std = @import("std");
const sys = @import("sys.zig");

const State = union(enum) {
    absent,
    dir,
    link: u64,
    file: struct { hash: u64, size: u64, mode: std.fs.File.Mode },

    fn eql(a: State, b: State) bool {
        return switch (a) {
            .absent => b == .absent,
            .dir => b == .dir,
            .link => |h| b == .link and b.link == h,
            .file => |f| b == .file and b.file.hash == f.hash and b.file.size == f.size and b.file.mode == f.mode,
        };
    }
};

fn stateOf(root: std.fs.Dir, p: []const u8) State {
    var buf: [std.fs.max_path_bytes]u8 = undefined;
    const st = root.statFile(p) catch |e| switch (e) {
        error.FileNotFound => return .absent,
        else => return .absent,
    };
    // statFile follows links; check for a link first.
    if (root.readLink(p, &buf)) |target| return .{ .link = std.hash.XxHash3.hash(0, target) } else |_| {}
    switch (st.kind) {
        .directory => return .dir,
        .file => {
            const f = root.openFile(p, .{}) catch return .absent;
            defer f.close();
            var h = std.hash.XxHash3.init(0);
            var chunk: [1 << 16]u8 = undefined;
            while (true) {
                const n = f.read(&chunk) catch break;
                if (n == 0) break;
                h.update(chunk[0..n]);
            }
            return .{ .file = .{ .hash = h.final(), .size = st.size, .mode = @intCast(st.mode & 0o7777) } };
        },
        else => return .absent,
    }
}

fn walk(gpa: std.mem.Allocator, root: std.fs.Dir, set: *std.StringArrayHashMap(void)) !void {
    var w = try root.walk(gpa);
    defer w.deinit();
    while (try w.next()) |e| {
        if (std.mem.startsWith(u8, e.path, "lost+found")) continue;
        try set.put(try gpa.dupe(u8, e.path), {});
    }
}

pub fn run(gpa: std.mem.Allocator, args: []const [:0]u8) !void {
    var before_p: []const u8 = "";
    var after_p: []const u8 = "";
    var live_p: []const u8 = "";
    var apply = false;
    for (args) |a| {
        if (std.mem.startsWith(u8, a, "before=")) before_p = a[7..];
        if (std.mem.startsWith(u8, a, "after=")) after_p = a[6..];
        if (std.mem.startsWith(u8, a, "live=")) live_p = a[5..];
        if (std.mem.eql(u8, a, "apply=1")) apply = true;
    }
    const before = try std.fs.cwd().openDir(before_p, .{ .iterate = true });
    const after = try std.fs.cwd().openDir(after_p, .{ .iterate = true });
    var live = try std.fs.cwd().openDir(live_p, .{ .iterate = true });
    const t0 = sys.now();

    // The paths the run changed.
    var all = std.StringArrayHashMap(void).init(gpa);
    try walk(gpa, before, &all);
    try walk(gpa, after, &all);
    var changed: std.ArrayList([]const u8) = .empty;
    for (all.keys()) |p| {
        if (!stateOf(before, p).eql(stateOf(after, p))) try changed.append(gpa, p);
    }
    const t1 = sys.now();

    // Put back what nobody touched since; show the rest. Deepest paths first for removals, so a
    // folder the run made is emptied before it is removed; shallowest first for creations.
    std.mem.sort([]const u8, changed.items, {}, struct {
        fn lt(_: void, a: []const u8, b: []const u8) bool {
            return std.mem.lessThan(u8, a, b);
        }
    }.lt);
    var put: u64 = 0;
    var removed: u64 = 0;
    var shown: std.ArrayList([]const u8) = .empty;
    var removals: std.ArrayList([]const u8) = .empty;
    for (changed.items) |p| {
        const sb = stateOf(before, p);
        const sa = stateOf(after, p);
        const sl = stateOf(live, p);
        if (!sl.eql(sa)) {
            try shown.append(gpa, p);
            continue;
        }
        switch (sb) {
            .absent => try removals.append(gpa, p),
            .dir => {
                if (apply) live.makePath(p) catch {};
                put += 1;
            },
            .file => |f| {
                if (apply) {
                    if (std.fs.path.dirname(p)) |d| live.makePath(d) catch {};
                    if (sl == .dir) live.deleteTree(p) catch {};
                    // Written to a temporary name and renamed, so a program in the computer never
                    // reads half a file.
                    var tmp_buf: [std.fs.max_path_bytes]u8 = undefined;
                    const tmp = try std.fmt.bufPrint(&tmp_buf, "{s}.silo-putback", .{p});
                    try before.copyFile(p, live, tmp, .{});
                    const tf = try live.openFile(tmp, .{ .mode = .read_write });
                    try tf.chmod(f.mode);
                    tf.close();
                    try live.rename(tmp, p);
                }
                put += 1;
            },
            .link => {
                var lb: [std.fs.max_path_bytes]u8 = undefined;
                const target = try before.readLink(p, &lb);
                if (apply) {
                    live.deleteFile(p) catch {};
                    try live.symLink(target, p, .{});
                }
                put += 1;
            },
        }
    }
    std.mem.reverse([]const u8, removals.items);
    for (removals.items) |p| {
        if (apply) {
            live.deleteFile(p) catch live.deleteDir(p) catch {};
        }
        removed += 1;
    }
    const t2 = sys.now();
    var ob: [4096]u8 = undefined;
    var ow = std.fs.File.stdout().writer(&ob);
    const out = &ow.interface;
    try out.print("{{\"paths_seen\":{d},\"changed_by_run\":{d},\"put_back\":{d},\"removed\":{d},\"shown_not_put_back\":{d},\"find_s\":{d:.2},\"put_s\":{d:.2},\"applied\":{}}}\n", .{ all.count(), changed.items.len, put, removed, shown.items.len, sys.secs(t1 - t0), sys.secs(t2 - t1), apply });
    for (shown.items) |p| try out.print("shown: {s}\n", .{p});
    try out.flush();
}
