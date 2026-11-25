//! Name lookups on the hand-off path. gvproxy, with `-dns-upstream`, sends the computer's lookups
//! here (#380). Each allowed name gets an address of its own from 198.18.0.0/15, which is set
//! aside for tests and is nobody's, so that a connection handed over to that address names its
//! host before any TLS, as a CONNECT does. A name that is not allowed gets "no such name", so the
//! lookup carries nothing out (#380: `x<data>.exfil.example` reaches the boundary only).
const std = @import("std");

pub const metadata_name = "metadata.google.internal";
pub const metadata_addr = [4]u8{ 169, 254, 169, 254 };

pub const Table = struct {
    mu: std.Thread.Mutex = .{},
    names: std.ArrayList([]const u8) = .empty,
    gpa: std.mem.Allocator,

    /// The address of an allowed name, given on first use and kept.
    pub fn addressOf(self: *Table, name: []const u8) ![4]u8 {
        self.mu.lock();
        defer self.mu.unlock();
        for (self.names.items, 0..) |n, i| if (std.ascii.eqlIgnoreCase(n, name)) return addr(i);
        if (self.names.items.len >= (1 << 17) - 2) return error.TableFull;
        try self.names.append(self.gpa, try self.gpa.dupe(u8, name));
        return addr(self.names.items.len - 1);
    }

    /// The name an address was given for, or null.
    pub fn nameOf(self: *Table, a: [4]u8) ?[]const u8 {
        if (a[0] != 198 or (a[1] != 18 and a[1] != 19)) return null;
        const i: usize = ((@as(usize, a[1]) - 18) << 16 | @as(usize, a[2]) << 8 | a[3]);
        if (i == 0) return null;
        self.mu.lock();
        defer self.mu.unlock();
        if (i - 1 >= self.names.items.len) return null;
        return self.names.items[i - 1];
    }

    fn addr(i: usize) [4]u8 {
        const k = i + 1;
        return .{ 198, @intCast(18 + (k >> 16)), @intCast((k >> 8) & 0xff), @intCast(k & 0xff) };
    }
};

pub const Query = struct { name: []const u8, qtype: u16, end: usize };

/// Reads the first question of a query. The name is written lower case into `buf`.
pub fn parseQuery(q: []const u8, buf: []u8) !Query {
    if (q.len < 12) return error.Short;
    if (q[2] & 0x80 != 0) return error.NotAQuery;
    if (std.mem.readInt(u16, q[4..6], .big) < 1) return error.NoQuestion;
    var p: usize = 12;
    var n: usize = 0;
    while (true) {
        if (p >= q.len) return error.Short;
        const l = q[p];
        if (l == 0) break;
        if (l & 0xc0 != 0) return error.Compressed;
        if (p + 1 + l > q.len or n + l + 1 > buf.len) return error.Short;
        if (n > 0) {
            buf[n] = '.';
            n += 1;
        }
        for (q[p + 1 .. p + 1 + l], 0..) |ch, k| buf[n + k] = std.ascii.toLower(ch);
        n += l;
        p += 1 + l;
    }
    p += 1;
    if (p + 4 > q.len) return error.Short;
    return .{ .name = buf[0..n], .qtype = std.mem.readInt(u16, q[p..][0..2], .big), .end = p + 4 };
}

/// The answer to a query: one A record when `a` is given and the question is A, no record for
/// another type of an allowed name, and "no such name" when `a` is null.
pub fn answer(q: []const u8, qr: Query, a: ?[4]u8, out: []u8) ![]u8 {
    if (qr.end + 16 > out.len) return error.Short;
    @memcpy(out[0..qr.end], q[0..qr.end]);
    out[2] = 0x80 | (q[2] & 0x01) | 0x04; // a response, recursion desired as asked, authoritative
    out[3] = 0x80 | @as(u8, if (a == null) 3 else 0);
    std.mem.writeInt(u16, out[4..6], 1, .big);
    std.mem.writeInt(u16, out[8..10], 0, .big);
    std.mem.writeInt(u16, out[10..12], 0, .big);
    if (a == null or qr.qtype != 1) {
        std.mem.writeInt(u16, out[6..8], 0, .big);
        return out[0..qr.end];
    }
    std.mem.writeInt(u16, out[6..8], 1, .big);
    const rr = out[qr.end .. qr.end + 16];
    @memcpy(rr[0..12], "\xc0\x0c\x00\x01\x00\x01\x00\x00\x00\x3c\x00\x04");
    @memcpy(rr[12..16], &a.?);
    return out[0 .. qr.end + 16];
}

const t = std.testing;

test "addresses of names" {
    var tb: Table = .{ .gpa = t.allocator };
    defer {
        for (tb.names.items) |n| t.allocator.free(n);
        tb.names.deinit(t.allocator);
    }
    const a = try tb.addressOf("api.anthropic.com");
    try t.expectEqual([4]u8{ 198, 18, 0, 1 }, a);
    try t.expectEqual([4]u8{ 198, 18, 0, 1 }, try tb.addressOf("API.anthropic.com"));
    try t.expectEqual([4]u8{ 198, 18, 0, 2 }, try tb.addressOf("github.com"));
    try t.expectEqualStrings("github.com", tb.nameOf(.{ 198, 18, 0, 2 }).?);
    try t.expectEqual(@as(?[]const u8, null), tb.nameOf(.{ 198, 18, 0, 3 }));
    try t.expectEqual(@as(?[]const u8, null), tb.nameOf(.{ 10, 0, 0, 1 }));
}

test "a query and its answers" {
    const q = "\x12\x34\x01\x00\x00\x01\x00\x00\x00\x00\x00\x00\x03api\x09Anthropic\x03com\x00\x00\x01\x00\x01";
    var nb: [256]u8 = undefined;
    const qr = try parseQuery(q, &nb);
    try t.expectEqualStrings("api.anthropic.com", qr.name);
    try t.expectEqual(@as(u16, 1), qr.qtype);
    var out: [512]u8 = undefined;
    const r = try answer(q, qr, .{ 198, 18, 0, 1 }, &out);
    try t.expectEqual(q.len + 16, r.len);
    try t.expectEqual(@as(u8, 0x80), r[3]);
    try t.expectEqualSlices(u8, &.{ 198, 18, 0, 1 }, r[r.len - 4 ..]);
    const nx = try answer(q, qr, null, &out);
    try t.expectEqual(@as(u8, 0x83), nx[3]);
    try t.expectEqual(q.len, nx.len);
}
