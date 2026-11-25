//! The two ways a connection reaches the boundary, as #380 recommends: a `CONNECT host:port` from
//! the relay in the computer (#378, over vsock), and a hand-off from gvproxy that starts with a
//! PROXY protocol v2 header (#380's `-outbound-handoff`). Both give a destination and a stream.
//! Also: the name a TLS ClientHello asks for, read without taking it off the socket, for a
//! connection that is passed through unread.
const std = @import("std");

pub const Dest = struct {
    /// The name, when the way in gives one (CONNECT, or an address the boundary's lookups gave).
    host: ?[]const u8 = null,
    /// The address the computer asked for, when the way in gives one (a hand-off).
    addr: ?std.net.Address = null,
    port: u16 = 0,
};

pub const Connect = struct {
    /// "CONNECT", or the method of a plain request in absolute form (`GET http://host/...`).
    method: []const u8,
    host: []const u8,
    port: u16,
    /// For a plain request: the path, from its first `/`.
    path: []const u8 = "",
    /// The bytes of the head, up to and including the blank line.
    head_len: usize,
};

pub const ParseError = error{ Incomplete, BadRequest, HeadTooLarge };

/// Reads a proxy request head: `CONNECT host:port HTTP/1.1`, or `METHOD http://host[:port]/path`.
pub fn parseConnect(buf: []const u8) ParseError!Connect {
    const end = (std.mem.indexOf(u8, buf, "\r\n\r\n") orelse {
        if (buf.len >= 8192) return error.HeadTooLarge;
        return error.Incomplete;
    }) + 4;
    const line_end = std.mem.indexOf(u8, buf, "\r\n").?;
    var it = std.mem.splitScalar(u8, buf[0..line_end], ' ');
    const method = it.next() orelse return error.BadRequest;
    const target = it.next() orelse return error.BadRequest;
    const version = it.next() orelse return error.BadRequest;
    if (!std.mem.startsWith(u8, version, "HTTP/1.")) return error.BadRequest;
    if (std.mem.eql(u8, method, "CONNECT")) {
        const hp = try splitHostPort(target, null);
        return .{ .method = method, .host = hp.host, .port = hp.port, .head_len = end };
    }
    if (!std.ascii.startsWithIgnoreCase(target, "http://")) return error.BadRequest;
    const rest = target[7..];
    const slash = std.mem.indexOfScalar(u8, rest, '/') orelse rest.len;
    const hp = try splitHostPort(rest[0..slash], 80);
    return .{ .method = method, .host = hp.host, .port = hp.port, .path = if (slash < rest.len) rest[slash..] else "/", .head_len = end };
}

fn splitHostPort(s: []const u8, default: ?u16) ParseError!struct { host: []const u8, port: u16 } {
    if (s.len == 0) return error.BadRequest;
    if (s[0] == '[') {
        const close = std.mem.indexOfScalar(u8, s, ']') orelse return error.BadRequest;
        const host = s[1..close];
        if (close + 1 == s.len) return .{ .host = host, .port = default orelse return error.BadRequest };
        if (s[close + 1] != ':') return error.BadRequest;
        return .{ .host = host, .port = std.fmt.parseInt(u16, s[close + 2 ..], 10) catch return error.BadRequest };
    }
    const colon = std.mem.lastIndexOfScalar(u8, s, ':') orelse return .{ .host = s, .port = default orelse return error.BadRequest };
    const port = std.fmt.parseInt(u16, s[colon + 1 ..], 10) catch return error.BadRequest;
    if (port == 0) return error.BadRequest;
    return .{ .host = s[0..colon], .port = port };
}

pub const Proxy = struct {
    src: std.net.Address,
    dst: std.net.Address,
    /// The whole header's length.
    len: usize,
};

const signature = "\x0d\x0a\x0d\x0a\x00\x0d\x0a\x51\x55\x49\x54\x0a";

/// Reads a PROXY protocol v2 header. Only PROXY over TCP, IPv4 or IPv6, is accepted: a LOCAL
/// command, UDP or a Unix address is refused, since gvproxy hands only TCP connections over.
pub fn parseProxyV2(buf: []const u8) ParseError!Proxy {
    if (buf.len < 16) return error.Incomplete;
    if (!std.mem.eql(u8, buf[0..12], signature)) return error.BadRequest;
    if (buf[12] != 0x21) return error.BadRequest; // version 2, PROXY
    const fam = buf[13];
    const len: usize = std.mem.readInt(u16, buf[14..16], .big);
    if (len > 512) return error.BadRequest;
    if (buf.len < 16 + len) return error.Incomplete;
    const a = buf[16 .. 16 + len];
    switch (fam) {
        0x11 => { // TCP over IPv4
            if (len < 12) return error.BadRequest;
            return .{
                .src = std.net.Address.initIp4(a[0..4].*, std.mem.readInt(u16, a[8..10], .big)),
                .dst = std.net.Address.initIp4(a[4..8].*, std.mem.readInt(u16, a[10..12], .big)),
                .len = 16 + len,
            };
        },
        0x21 => { // TCP over IPv6
            if (len < 36) return error.BadRequest;
            return .{
                .src = std.net.Address.initIp6(a[0..16].*, std.mem.readInt(u16, a[32..34], .big), 0, 0),
                .dst = std.net.Address.initIp6(a[16..32].*, std.mem.readInt(u16, a[34..36], .big), 0, 0),
                .len = 16 + len,
            };
        },
        else => return error.BadRequest,
    }
}

/// The server name of a TLS ClientHello, or null when the record is whole and names none.
/// error.Incomplete: more bytes are needed. error.BadRequest: not a ClientHello.
pub fn sniOf(buf: []const u8) ParseError!?[]const u8 {
    if (buf.len < 5) return error.Incomplete;
    if (buf[0] != 0x16) return error.BadRequest;
    const rec_len: usize = std.mem.readInt(u16, buf[3..5], .big);
    if (buf.len < 5 + rec_len) return error.Incomplete;
    var p = buf[5 .. 5 + rec_len];
    if (p.len < 4 or p[0] != 0x01) return error.BadRequest;
    p = p[4..];
    if (p.len < 34) return error.BadRequest;
    p = p[34..]; // version, random
    p = try skip8(p); // session id
    p = try skip16(p); // cipher suites
    p = try skip8(p); // compression
    if (p.len < 2) return null;
    const ext_len: usize = std.mem.readInt(u16, p[0..2], .big);
    if (p.len < 2 + ext_len) return error.BadRequest;
    var e = p[2 .. 2 + ext_len];
    while (e.len >= 4) {
        const typ = std.mem.readInt(u16, e[0..2], .big);
        const l: usize = std.mem.readInt(u16, e[2..4], .big);
        if (e.len < 4 + l) return error.BadRequest;
        if (typ == 0) {
            const d = e[4 .. 4 + l];
            if (d.len < 5 or d[2] != 0) return error.BadRequest;
            const nl: usize = std.mem.readInt(u16, d[3..5], .big);
            if (d.len < 5 + nl) return error.BadRequest;
            return d[5 .. 5 + nl];
        }
        e = e[4 + l ..];
    }
    return null;
}

fn skip8(p: []const u8) ParseError![]const u8 {
    if (p.len < 1 or p.len < 1 + @as(usize, p[0])) return error.BadRequest;
    return p[1 + @as(usize, p[0]) ..];
}
fn skip16(p: []const u8) ParseError![]const u8 {
    if (p.len < 2) return error.BadRequest;
    const l: usize = std.mem.readInt(u16, p[0..2], .big);
    if (p.len < 2 + l) return error.BadRequest;
    return p[2 + l ..];
}

const t = std.testing;

test "CONNECT and absolute-form requests" {
    const a = try parseConnect("CONNECT api.anthropic.com:443 HTTP/1.1\r\nHost: api.anthropic.com:443\r\n\r\n\x16\x03");
    try t.expectEqualStrings("api.anthropic.com", a.host);
    try t.expectEqual(@as(u16, 443), a.port);
    try t.expectEqual(@as(usize, 71), a.head_len);
    const b = try parseConnect("GET http://169.254.169.254/computeMetadata/v1/?recursive=true HTTP/1.1\r\nMetadata-Flavor: Google\r\n\r\n");
    try t.expectEqualStrings("169.254.169.254", b.host);
    try t.expectEqual(@as(u16, 80), b.port);
    try t.expectEqualStrings("/computeMetadata/v1/?recursive=true", b.path);
    const v6 = try parseConnect("CONNECT [2001:db8::1]:22 HTTP/1.1\r\n\r\n");
    try t.expectEqualStrings("2001:db8::1", v6.host);
    try t.expectError(error.Incomplete, parseConnect("CONNECT a:443 HTTP/1.1\r\n"));
    try t.expectError(error.BadRequest, parseConnect("CONNECT a HTTP/1.1\r\n\r\n"));
    try t.expectError(error.BadRequest, parseConnect("CONNECT a:0 HTTP/1.1\r\n\r\n"));
    try t.expectError(error.BadRequest, parseConnect("GET /x HTTP/1.1\r\n\r\n"));
    try t.expectError(error.BadRequest, parseConnect("SSH-2.0-OpenSSH_9.6\r\n\r\n"));
}

test "PROXY v2" {
    const h = signature ++ "\x21\x11\x00\x0c" ++ "\xc0\xa8\x7f\x02" ++ "\xc6\x12\x00\x05" ++ "\xd4\x31" ++ "\x01\xbb" ++ "GET";
    const p = try parseProxyV2(h);
    try t.expectEqual(@as(usize, 28), p.len);
    try t.expectEqual(@as(u16, 443), p.dst.getPort());
    try t.expectEqual(@as(u16, 54321), p.src.getPort());
    try t.expectEqual([4]u8{ 198, 18, 0, 5 }, @as([4]u8, @bitCast(p.dst.in.sa.addr)));
    try t.expectError(error.Incomplete, parseProxyV2(h[0..20]));
    try t.expectError(error.BadRequest, parseProxyV2("PROXY TCP4 1.2.3.4 5.6.7.8 1 2\r\n"));
    try t.expectError(error.BadRequest, parseProxyV2(signature ++ "\x20\x00\x00\x00")); // LOCAL
    try t.expectError(error.BadRequest, parseProxyV2(signature ++ "\x21\x12\x00\x0c" ++ "\x00" ** 12)); // UDP
}

test "SNI of a ClientHello" {
    // A minimal ClientHello naming "github.com".
    const name = "github.com";
    const sni_ext = "\x00\x00" ++ "\x00\x0f" ++ "\x00\x0d" ++ "\x00" ++ "\x00\x0a" ++ name;
    const body = "\x03\x03" ++ ("\x00" ** 32) ++ "\x00" ++ "\x00\x02\x13\x01" ++ "\x01\x00" ++ "\x00\x13" ++ sni_ext;
    const hs = "\x01\x00\x00" ++ [_]u8{@intCast(body.len)} ++ body;
    const rec = "\x16\x03\x01\x00" ++ [_]u8{@intCast(hs.len)} ++ hs;
    try t.expectEqualStrings(name, (try sniOf(rec)).?);
    try t.expectError(error.Incomplete, sniOf(rec[0 .. rec.len - 1]));
    try t.expectError(error.BadRequest, sniOf("SSH-2.0-x\r\n"));
}
