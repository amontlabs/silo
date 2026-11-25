//! Takes the real value of every secret out of what reaches the record (`secret-never-written-down`).
//! The value is replaced by `[secret:<name>]` before the line is hashed, so the chain is over the
//! redacted bytes and the value is never on disk.
const std = @import("std");

pub const min_value_len = 8;

pub const Secret = struct { name: []const u8, value: []const u8 };

pub const Redactor = struct {
    secrets: []const Secret = &.{},
    /// One bit per pair of bytes that starts a secret, so most places are passed over with one
    /// lookup whatever the number of secrets.
    starts: ?*[65536 / 8]u8 = null,

    pub const Error = error{ OutOfMemory, SecretTooShort };

    /// Checks the secrets once, when they are given. A value shorter than `min_value_len` would
    /// cut ordinary words out of the record.
    pub fn init(secrets: []const Secret) Error!Redactor {
        for (secrets) |s| if (s.value.len < min_value_len) return error.SecretTooShort;
        if (secrets.len == 0) return .{};
        const t = try std.heap.page_allocator.create([65536 / 8]u8);
        @memset(t, 0);
        for (secrets) |s| {
            const k = @as(u16, s.value[0]) << 8 | s.value[1];
            t[k >> 3] |= @as(u8, 1) << @intCast(k & 7);
        }
        return .{ .secrets = secrets, .starts = t };
    }

    /// Appends `in` to `out` with every secret value replaced. Returns how many were replaced.
    pub fn apply(r: Redactor, gpa: std.mem.Allocator, out: *std.ArrayList(u8), in: []const u8) error{OutOfMemory}!usize {
        var count: usize = 0;
        var i: usize = 0;
        outer: while (i < in.len) {
            // The longest secret that starts here wins, so a secret that holds another is taken whole.
            var best: ?usize = null;
            if (r.starts) |t| {
                const k = if (i + 1 < in.len) @as(u16, in[i]) << 8 | in[i + 1] else 0;
                if (i + 1 >= in.len or t[k >> 3] & (@as(u8, 1) << @intCast(k & 7)) == 0) {
                    try out.append(gpa, in[i]);
                    i += 1;
                    continue :outer;
                }
            }
            for (r.secrets, 0..) |s, k| {
                if (in.len - i >= s.value.len and in[i] == s.value[0] and std.mem.eql(u8, in[i .. i + s.value.len], s.value)) {
                    if (best == null or s.value.len > r.secrets[best.?].value.len) best = k;
                }
            }
            if (best) |k| {
                try out.appendSlice(gpa, "[secret:");
                try out.appendSlice(gpa, r.secrets[k].name);
                try out.append(gpa, ']');
                i += r.secrets[k].value.len;
                count += 1;
                continue :outer;
            }
            try out.append(gpa, in[i]);
            i += 1;
        }
        return count;
    }
};

test "a value is replaced wherever it appears" {
    const gpa = std.testing.allocator;
    const r = try Redactor.init(&.{
        .{ .name = "stripe-test", .value = "sk_test_51Hx9aZ" },
        .{ .name = "gh", .value = "ghp_abcdefgh" },
    });
    var out: std.ArrayList(u8) = .empty;
    defer out.deinit(gpa);
    const n = try r.apply(gpa, &out, "curl -H 'Authorization: Bearer sk_test_51Hx9aZ' x?t=ghp_abcdefghghp_abcdefgh");
    try std.testing.expectEqual(@as(usize, 3), n);
    try std.testing.expectEqualStrings("curl -H 'Authorization: Bearer [secret:stripe-test]' x?t=[secret:gh][secret:gh]", out.items);
}

test "a short value is refused" {
    try std.testing.expectError(error.SecretTooShort, Redactor.init(&.{.{ .name = "pin", .value = "1234" }}));
}

test "the longest secret wins" {
    const gpa = std.testing.allocator;
    const r = try Redactor.init(&.{
        .{ .name = "short", .value = "abcdefgh" },
        .{ .name = "long", .value = "abcdefgh-ijkl" },
    });
    var out: std.ArrayList(u8) = .empty;
    defer out.deinit(gpa);
    _ = try r.apply(gpa, &out, "x abcdefgh-ijkl abcdefgh");
    try std.testing.expectEqualStrings("x [secret:long] [secret:short]", out.items);
}
