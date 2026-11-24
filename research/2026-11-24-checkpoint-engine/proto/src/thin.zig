//! Thinning, as `thinning-checkpoints` proposes it: every automatic checkpoint of the last 2 hours,
//! one per hour for 2 days, one per day for 30 days. The one kept for an hour or a day is the
//! oldest still there, so the rule never needs a checkpoint it removed before: thinning often or
//! rarely, or after a crash, gives the same set (the test below checks this on random histories).
//!
//! `ckpt thin` runs weeks of simulated work and prints how many checkpoints are kept, how many
//! are removed each hour, and the disk they hold under a model of writes.
const std = @import("std");

pub const Kind = enum { auto, turn, manual, before_restore };

pub const Ckpt = struct { t: i64, kind: Kind };

pub const Policy = struct {
    all_s: i64 = 2 * 3600,
    hourly_s: i64 = 2 * 86400,
    daily_s: i64 = 30 * 86400,
    /// Local time's offset from UTC, so that a day ends at local midnight.
    offset_s: i64 = 0,
};

fn thinned(k: Kind) bool {
    // Only checkpoints Silo took on its own are thinned. One you made, and the save made before a
    // restore (the only copy of what the restore left), are not.
    return k == .auto or k == .turn;
}

/// Marks in `keep` the checkpoints the rule keeps at `now`. `c` is sorted by time.
pub fn decide(p: Policy, now: i64, c: []const Ckpt, keep: []bool) void {
    var last_hour: i64 = std.math.minInt(i64);
    var last_day: i64 = std.math.minInt(i64);
    for (c, 0..) |x, i| {
        keep[i] = true;
        if (!thinned(x.kind)) continue;
        const age = now - x.t;
        const hour = @divFloor(x.t + p.offset_s, 3600);
        const day = @divFloor(x.t + p.offset_s, 86400);
        const first_of_hour = hour != last_hour;
        const first_of_day = day != last_day;
        last_hour = hour;
        last_day = day;
        if (age <= p.all_s) continue;
        if (age <= p.hourly_s and first_of_hour) continue;
        if (age <= p.daily_s and first_of_day) continue;
        keep[i] = false;
    }
}

/// Removes from `c` what the rule does not keep; returns how many it removed.
pub fn apply(gpa: std.mem.Allocator, p: Policy, now: i64, c: *std.ArrayList(Ckpt)) !usize {
    const keep = try gpa.alloc(bool, c.items.len);
    defer gpa.free(keep);
    decide(p, now, c.items, keep);
    var j: usize = 0;
    for (c.items, keep) |x, k| {
        if (k) {
            c.items[j] = x;
            j += 1;
        }
    }
    const removed = c.items.len - j;
    c.shrinkRetainingCapacity(j);
    return removed;
}

test "thinning gives the same set however often it runs" {
    const gpa = std.testing.allocator;
    var prng = std.Random.DefaultPrng.init(42);
    const r = prng.random();
    var round: usize = 0;
    while (round < 300) : (round += 1) {
        const p = Policy{
            .all_s = r.intRangeAtMost(i64, 600, 4 * 3600),
            .hourly_s = r.intRangeAtMost(i64, 5 * 3600, 3 * 86400),
            .daily_s = r.intRangeAtMost(i64, 4 * 86400, 40 * 86400),
            .offset_s = 3600 * r.intRangeAtMost(i64, -11, 12),
        };
        var inc: std.ArrayList(Ckpt) = .empty;
        defer inc.deinit(gpa);
        var all: std.ArrayList(Ckpt) = .empty;
        defer all.deinit(gpa);
        var t: i64 = 1_700_000_000 + r.intRangeAtMost(i64, 0, 86400);
        const end = t + 50 * 86400;
        while (t < end) {
            t += r.intRangeAtMost(i64, 1, 3 * 3600);
            const k: Kind = switch (r.uintLessThan(u8, 20)) {
                0 => .manual,
                1 => .before_restore,
                2...9 => .turn,
                else => .auto,
            };
            try inc.append(gpa, .{ .t = t, .kind = k });
            try all.append(gpa, .{ .t = t, .kind = k });
            if (r.uintLessThan(u8, 4) == 0) _ = try apply(gpa, p, t, &inc);
        }
        _ = try apply(gpa, p, t, &inc);
        _ = try apply(gpa, p, t, &all);
        try std.testing.expectEqualSlices(Ckpt, all.items, inc.items);
        // The newest is always kept.
        try std.testing.expectEqual(all.items[all.items.len - 1].t, t);
    }
}

pub const SimOpts = struct {
    days: u64 = 40,
    active_h: u64 = 10, // hours of work a day
    interval_s: u64 = 300, // automatic checkpoints while work goes on
    turns_per_min: f64 = 0, // checkpoints at each turn (`checkpoint-at-each-turn`), all agents together
    thin_every_s: u64 = 60,
    // The model of writes, for the disk the checkpoints hold: `gib_per_h` of the disk rewritten each
    // hour of work, in pieces of 1 MiB, 80% of them in a working set of `hot_gib`.
    disk_gib: u64 = 64,
    hot_gib: u64 = 8,
    gib_per_h: f64 = 2,
    seed: u64 = 1,
    keep_all_h: i64 = 2,
    hourly_days: i64 = 2,
    daily_days: i64 = 30,
};

const Version = struct { birth: i64, death: i64 };

pub fn sim(gpa: std.mem.Allocator, o: SimOpts) !void {
    var obuf: [4096]u8 = undefined;
    var ow = std.fs.File.stdout().writerStreaming(&obuf);
    const out = &ow.interface;
    const p = Policy{ .all_s = o.keep_all_h * 3600, .hourly_s = o.hourly_days * 86400, .daily_s = o.daily_days * 86400 };
    var prng = std.Random.DefaultPrng.init(o.seed);
    const r = prng.random();
    var c: std.ArrayList(Ckpt) = .empty;
    const blocks = o.disk_gib * 1024;
    const hot = o.hot_gib * 1024;
    const born = try gpa.alloc(i64, blocks);
    @memset(born, 0);
    var dead: std.ArrayList(Version) = .empty;
    const writes_per_s = o.gib_per_h * 1024 / 3600;
    var wacc: f64 = 0;
    var t: i64 = 0;
    const end: i64 = @intCast(o.days * 86400);
    var next_ckpt: i64 = 0;
    var next_turn: i64 = 0;
    var removed_total: usize = 0;
    var max_kept: usize = 0;
    var removed_hour: usize = 0;
    var max_removed_hour: usize = 0;
    var taken: usize = 0;
    var hour_mark: i64 = 0;
    while (t < end) : (t += 1) {
        const in_day = @mod(t, 86400);
        const active = in_day >= 9 * 3600 and in_day < @as(i64, @intCast(9 + o.active_h)) * 3600;
        if (active) {
            wacc += writes_per_s;
            while (wacc >= 1) : (wacc -= 1) {
                const b = if (r.uintLessThan(u8, 10) < 8) r.uintLessThan(u64, hot) else r.uintLessThan(u64, blocks);
                try dead.append(gpa, .{ .birth = born[b], .death = t });
                born[b] = t;
            }
            if (t >= next_ckpt) {
                try c.append(gpa, .{ .t = t, .kind = .auto });
                taken += 1;
                next_ckpt = t + @as(i64, @intCast(o.interval_s));
            }
            if (o.turns_per_min > 0 and t >= next_turn) {
                try c.append(gpa, .{ .t = t, .kind = .turn });
                taken += 1;
                const mean = 60.0 / o.turns_per_min;
                next_turn = t + @as(i64, @intFromFloat(@max(1, -mean * @log(1 - r.float(f64)))));
            }
        } else if (c.items.len > 0 and t == next_ckpt) {
            // Nothing is written while nobody works: no checkpoint would differ from the last one.
            next_ckpt = t + @as(i64, @intCast(o.interval_s));
        }
        if (@mod(t, @as(i64, @intCast(o.thin_every_s))) == 0) {
            const n = try apply(gpa, p, t, &c);
            removed_total += n;
            removed_hour += n;
        }
        if (t - hour_mark >= 3600) {
            max_removed_hour = @max(max_removed_hour, removed_hour);
            removed_hour = 0;
            hour_mark = t;
        }
        max_kept = @max(max_kept, c.items.len);
    }
    // The disk the kept checkpoints hold beyond the live disk: every old version of a piece that a
    // kept checkpoint still sees (born at or before it, overwritten after it).
    var extra: u64 = 0;
    for (dead.items) |v| {
        // the first kept checkpoint at or after birth
        var lo: usize = 0;
        var hi: usize = c.items.len;
        while (lo < hi) {
            const mid = (lo + hi) / 2;
            if (c.items[mid].t < v.birth) lo = mid + 1 else hi = mid;
        }
        if (lo < c.items.len and c.items[lo].t < v.death) extra += 1;
    }
    try out.print("{{\"daily_days\":{d},\"days\":{d},\"interval_s\":{d},\"turns_per_min\":{d:.1},\"gib_per_h\":{d:.1},\"hot_gib\":{d},\"taken\":{d},\"kept_now\":{d},\"kept_max\":{d},\"removed_max_per_hour\":{d},\"kept_disk_gib\":{d:.1}}}\n", .{ o.daily_days, o.days, o.interval_s, o.turns_per_min, o.gib_per_h, o.hot_gib, taken, c.items.len, max_kept, max_removed_hour, @as(f64, @floatFromInt(extra)) / 1024 });
    try out.flush();
}
