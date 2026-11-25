//! The reader's fuzzer. It changes a real record at random, many times, and asks verify each time.
//! Two properties: verify never crashes, whatever bytes it reads; and verify never says a changed
//! record holds. With the head, every change is found, including a cut at a line's end.
const std = @import("std");
const tamper = @import("tamper.zig");
const verify = @import("verify.zig");
const format = @import("format.zig");

pub const Stats = struct {
    runs: u64 = 0,
    same_bytes: u64 = 0,
    found: u64 = 0,
    missed: u64 = 0,
    /// How many runs ended on each message, by message.
    by_what: std.StringArrayHashMapUnmanaged(u64) = .empty,
    first_miss_seed: ?u64 = null,
};

const Op = enum { flip_bit, set_byte, truncate, delete_range, insert_random, duplicate_range, drop_segment, swap_segments, set_length, random_segment, payload_with_crc, payload_with_crc2 };

const tokens = "{}[]\",:0123456789aeflnrstu\\ ";

fn mutate(gpa: std.mem.Allocator, rng: std.Random, r: *tamper.Rec) !void {
    const op = rng.enumValue(Op);
    const n = r.segs.items.len;
    if (n == 0) return;
    const si = rng.uintLessThan(usize, n);
    const seg = r.segs.items[si];
    switch (op) {
        .flip_bit => if (seg.len > 0) {
            seg[rng.uintLessThan(usize, seg.len)] ^= @as(u8, 1) << rng.int(u3);
        },
        .set_byte => if (seg.len > 0) {
            seg[rng.uintLessThan(usize, seg.len)] = rng.int(u8);
        },
        .truncate => {
            const keep = rng.uintLessThan(usize, seg.len + 1);
            r.segs.items[si] = try gpa.realloc(seg, keep);
        },
        .delete_range => if (seg.len > 1) {
            const a = rng.uintLessThan(usize, seg.len);
            const l = 1 + rng.uintLessThan(usize, @min(seg.len - a, 300));
            const out = try std.mem.concat(gpa, u8, &.{ seg[0..a], seg[a + l ..] });
            gpa.free(seg);
            r.segs.items[si] = out;
        },
        .insert_random => {
            var junk: [64]u8 = undefined;
            const l = 1 + rng.uintLessThan(usize, junk.len);
            rng.bytes(junk[0..l]);
            const a = rng.uintLessThan(usize, seg.len + 1);
            const out = try std.mem.concat(gpa, u8, &.{ seg[0..a], junk[0..l], seg[a..] });
            gpa.free(seg);
            r.segs.items[si] = out;
        },
        .duplicate_range => if (seg.len > 1) {
            const a = rng.uintLessThan(usize, seg.len);
            const l = 1 + rng.uintLessThan(usize, @min(seg.len - a, 600));
            const out = try std.mem.concat(gpa, u8, &.{ seg[0 .. a + l], seg[a..] });
            gpa.free(seg);
            r.segs.items[si] = out;
        },
        .drop_segment => gpa.free(r.segs.orderedRemove(si)),
        .swap_segments => if (n > 1) {
            const sj = rng.uintLessThan(usize, n);
            std.mem.swap([]u8, &r.segs.items[si], &r.segs.items[sj]);
        },
        .set_length => {
            var fs = try tamper.frames(gpa, seg);
            defer fs.deinit(gpa);
            if (fs.items.len > 0) {
                const f = fs.items[rng.uintLessThan(usize, fs.items.len)];
                const v: u32 = switch (rng.uintLessThan(u8, 4)) {
                    0 => 0,
                    1 => 0xffff_ffff,
                    2 => @intCast(format.max_line + 1),
                    else => rng.int(u32) % 4096,
                };
                std.mem.writeInt(u32, seg[f.off..][0..4], v, .little);
            }
        },
        // A changed payload with a CRC that matches it, so the bytes reach the JSON reader and the
        // line's checks. Twice as likely as the others.
        .payload_with_crc, .payload_with_crc2 => {
            var fs = try tamper.frames(gpa, seg);
            defer fs.deinit(gpa);
            if (fs.items.len > 0) {
                const f = fs.items[rng.uintLessThan(usize, fs.items.len)];
                var p: std.ArrayList(u8) = .empty;
                defer p.deinit(gpa);
                try p.appendSlice(gpa, seg[f.off + 8 .. f.off + f.len]);
                const edits = 1 + rng.uintLessThan(u8, 4);
                for (0..edits) |_| {
                    if (p.items.len == 0) break;
                    const at = rng.uintLessThan(usize, p.items.len);
                    switch (rng.uintLessThan(u8, 5)) {
                        0 => p.items[at] = rng.int(u8),
                        1 => p.items[at] = tokens[rng.uintLessThan(usize, tokens.len)],
                        2 => _ = p.orderedRemove(at),
                        3 => try p.insert(gpa, at, tokens[rng.uintLessThan(usize, tokens.len)]),
                        else => p.shrinkRetainingCapacity(at),
                    }
                }
                const nf = try std.mem.concat(gpa, u8, &.{ seg[0..f.off], &format.frameHeader(p.items), p.items, seg[f.off + f.len ..] });
                gpa.free(seg);
                r.segs.items[si] = nf;
            }
        },
        .random_segment => {
            const l = rng.uintLessThan(usize, 2048);
            const out = try gpa.alloc(u8, l);
            rng.bytes(out);
            // Half the time with a valid header, so the walk reaches the frames.
            if (l >= format.header_size and seg.len >= format.header_size and rng.boolean()) out[0..format.header_size].* = seg[0..format.header_size].*;
            gpa.free(seg);
            r.segs.items[si] = out;
        },
    }
}

fn same(a: tamper.Rec, b: tamper.Rec) bool {
    if (a.segs.items.len != b.segs.items.len) return false;
    for (a.segs.items, b.segs.items) |x, y| if (!std.mem.eql(u8, x, y)) return false;
    return true;
}

pub fn run(gpa: std.mem.Allocator, base: tamper.Rec, runs: u64, seed: u64, st: *Stats) !void {
    var prng = std.Random.DefaultPrng.init(seed);
    const rng = prng.random();
    var i: u64 = 0;
    while (i < runs) : (i += 1) {
        var r = try base.clone();
        defer r.deinit();
        const ops = 1 + rng.uintLessThan(u8, 3);
        for (0..ops) |_| try mutate(gpa, rng, &r);
        st.runs += 1;
        if (same(r, base)) {
            st.same_bytes += 1;
            continue;
        }
        const res = r.check(true, null);
        if (res.ok()) {
            st.missed += 1;
            if (st.first_miss_seed == null) st.first_miss_seed = i;
        } else {
            st.found += 1;
            const g = try st.by_what.getOrPut(gpa, res.problem.?.what);
            if (!g.found_existing) g.value_ptr.* = 0;
            g.value_ptr.* += 1;
        }
    }
}

test "fuzz: verify finds every change and never crashes" {
    const gpa = std.testing.allocator;
    const path = "zig-cache-fuzz-test";
    std.fs.cwd().deleteTree(path) catch {};
    defer std.fs.cwd().deleteTree(path) catch {};
    {
        var w = try @import("writer.zig").Writer.open(gpa, path, "c1", @import("harness.zig").options(2048));
        defer w.close();
        try @import("harness.zig").workload(gpa, &w, null);
    }
    var dir = try std.fs.cwd().openDir(path, .{ .iterate = true });
    defer dir.close();
    var base = try tamper.Rec.load(gpa, dir);
    defer base.deinit();
    var st = Stats{};
    defer st.by_what.deinit(gpa);
    try run(gpa, base, 3000, 1, &st);
    try std.testing.expectEqual(@as(u64, 0), st.missed);
}
