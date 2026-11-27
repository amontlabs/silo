//! "Key or plan" for one computer (#393, `claude-key-or-plan`): how Claude signs in there decides
//! whether the boundary reads api.anthropic.com at all. A plan and an API key share that host, so
//! the boundary cannot tell them apart before it ends TLS; the choice is the computer's.
//!
//!   "read"  (absent) today's answer: every Anthropic connection is read, whatever it carries.
//!   "key"   read; a request that carries a plan's sign-in is refused and never leaves.
//!   "plan"  not read: the connection is carried as bytes, as a host Silo passes through.
//!
//! #393 built this into #374's boundary, where the host came from the ClientHello. Here the host
//! is already known from the way in (`CONNECT`, or the hand-off's address), so only the two
//! functions below are kept.
const std = @import("std");

pub const Mode = enum { read, key, plan };

pub fn modeOf(s: []const u8) Mode {
    if (std.mem.eql(u8, s, "key")) return .key;
    if (std.mem.eql(u8, s, "plan")) return .plan;
    return .read;
}

pub const SignIn = enum { key, plan, none };

/// The kind of sign-in a request to api.anthropic.com carries, from its head alone. Claude Code
/// sends an API key in `x-api-key` and a plan's sign-in as `Authorization: Bearer`. Any bearer
/// counts as a plan: in a computer set for a key, only `x-api-key` leaves.
pub fn signInOf(head: []const u8) SignIn {
    var it = std.mem.splitSequence(u8, head, "\r\n");
    _ = it.next();
    var key = false;
    while (it.next()) |line| {
        const colon = std.mem.indexOfScalar(u8, line, ':') orelse continue;
        const name = std.mem.trim(u8, line[0..colon], " ");
        if (std.ascii.eqlIgnoreCase(name, "authorization")) return .plan;
        if (std.ascii.eqlIgnoreCase(name, "x-api-key")) key = true;
    }
    return if (key) .key else .none;
}

test "sign-in kinds" {
    try std.testing.expectEqual(SignIn.key, signInOf("POST /v1/messages HTTP/1.1\r\nx-api-key: sk-ant-api03-x\r\n\r\n"));
    try std.testing.expectEqual(SignIn.plan, signInOf("POST /v1/messages HTTP/1.1\r\nAuthorization: Bearer sk-ant-oat01-x\r\n\r\n"));
    try std.testing.expectEqual(SignIn.plan, signInOf("POST /v1/messages HTTP/1.1\r\nx-api-key: k\r\nauthorization: Bearer other\r\n\r\n"));
    try std.testing.expectEqual(SignIn.none, signInOf("GET / HTTP/1.1\r\nHost: api.anthropic.com\r\n\r\n"));
}
