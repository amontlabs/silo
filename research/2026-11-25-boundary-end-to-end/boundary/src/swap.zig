//! Placeholders out, values in: on the way to a host a secret is allowed to, every placeholder of
//! the computer's secrets in the request's head (its request line and header values) is replaced
//! by the value. A placeholder in a head bound for another host is refused, as
//! `secret-placeholder` asks. A placeholder in a body is not swapped, only reported: #379 shows the
//! one case that needs it, a refresh token at a token endpoint.
const std = @import("std");
const conf = @import("conf.zig");

pub const Outcome = union(enum) {
    /// The head to send, and the secrets swapped into it (indexes into the computer's list).
    send: struct { head: []u8, used: u32 },
    /// A placeholder bound for a host it may not go to.
    elsewhere: usize,
};

/// `used` is a bit set of the secrets swapped in, by their index (up to 32 secrets).
pub fn swapHead(gpa: std.mem.Allocator, head: []const u8, host: []const u8, secrets: []const conf.Secret) !Outcome {
    var used: u32 = 0;
    for (secrets, 0..) |s, i| {
        if (s.placeholder.len == 0 or std.mem.indexOf(u8, head, s.placeholder) == null) continue;
        if (!allowed(s, host)) return .{ .elsewhere = i };
        used |= @as(u32, 1) << @intCast(i);
    }
    if (used == 0) return .{ .send = .{ .head = try gpa.dupe(u8, head), .used = 0 } };
    var cur = try gpa.dupe(u8, head);
    for (secrets, 0..) |s, i| {
        if (used & (@as(u32, 1) << @intCast(i)) == 0) continue;
        const next = try std.mem.replaceOwned(u8, gpa, cur, s.placeholder, s.value);
        gpa.free(cur);
        cur = next;
    }
    return .{ .send = .{ .head = cur, .used = used } };
}

pub fn allowed(s: conf.Secret, host: []const u8) bool {
    for (s.hosts) |h| if (std.ascii.eqlIgnoreCase(h, host)) return true;
    return false;
}

/// The first secret whose placeholder a body carries, if any.
pub fn inBody(body: []const u8, secrets: []const conf.Secret) ?usize {
    for (secrets, 0..) |s, i| if (s.placeholder.len > 0 and std.mem.indexOf(u8, body, s.placeholder) != null) return i;
    return null;
}

/// The request line with its query left out, for the record: a query can carry a key
/// (Gemini's `?key=`).
pub fn lineForRecord(first: []const u8) []const u8 {
    const q = std.mem.indexOfScalar(u8, first, '?') orelse return first;
    return first[0..q];
}

const t = std.testing;

test "swap only where allowed" {
    var hosts_a = [_][]const u8{"api.anthropic.com"};
    var hosts_g = [_][]const u8{ "storage.googleapis.com", "oauth2.googleapis.com" };
    const secrets = [_]conf.Secret{
        .{ .name = "anthropic", .placeholder = "sk-ant-api03-PH1", .value = "sk-ant-api03-REALREALREAL", .hosts = &hosts_a },
        .{ .name = "gcp", .placeholder = "ya29.PH2", .value = "ya29.REAL", .hosts = &hosts_g },
    };
    const head = "POST /v1/messages HTTP/1.1\r\nx-api-key: sk-ant-api03-PH1\r\n\r\n";
    const o = try swapHead(t.allocator, head, "api.anthropic.com", &secrets);
    defer t.allocator.free(o.send.head);
    try t.expectEqualStrings("POST /v1/messages HTTP/1.1\r\nx-api-key: sk-ant-api03-REALREALREAL\r\n\r\n", o.send.head);
    try t.expectEqual(@as(u32, 1), o.send.used);
    const e = try swapHead(t.allocator, head, "evil.example", &secrets);
    try t.expectEqual(@as(usize, 0), e.elsewhere);
    const n = try swapHead(t.allocator, "GET / HTTP/1.1\r\n\r\n", "evil.example", &secrets);
    defer t.allocator.free(n.send.head);
    try t.expectEqual(@as(u32, 0), n.send.used);
    const q = try swapHead(t.allocator, "GET /v1beta/models?key=ya29.PH2 HTTP/1.1\r\n\r\n", "storage.googleapis.com", &secrets);
    defer t.allocator.free(q.send.head);
    try t.expectEqualStrings("GET /v1beta/models?key=ya29.REAL HTTP/1.1\r\n\r\n", q.send.head);
    try t.expectEqualStrings("GET /v1beta/models", lineForRecord("GET /v1beta/models?key=ya29.PH2"));
    try t.expectEqual(@as(?usize, 1), inBody("{\"refresh_token\":\"ya29.PH2\"}", &secrets));
}
