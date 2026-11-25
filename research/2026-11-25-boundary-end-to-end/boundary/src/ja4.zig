//! A name for the TLS client of a connection, from its ClientHello alone, in the shape of FoxIO's
//! JA4 (`t13d1516h2_8daaf6152771_e5627efa2ab1`). Research of 25 November 2026, TLS refusals: a
//! program that refuses Silo's certificate never sends a request, so its User-Agent is never
//! seen; the ClientHello is all the boundary has to tell one program from another.
//!
//! Ciphers and extensions are sorted before hashing, as JA4 does, so a client that shuffles its
//! extensions (Chrome, and BoringSSL builds such as Bun's) keeps one name.
const std = @import("std");

pub const Hello = struct {
    /// The highest version in supported_versions, or the legacy version when that is absent.
    version: u16,
    has_sni: bool,
    ciphers: []const u16,
    extensions: []const u16,
    /// The first protocol the client offers in ALPN, empty when it offers none.
    alpn: []const u8,
    sigalgs: []const u16,
};

pub fn isGrease(v: u16) bool {
    return (v & 0x0f0f) == 0x0a0a and (v >> 8) == (v & 0xff);
}

fn versionCode(v: u16) []const u8 {
    return switch (v) {
        0x0304 => "13",
        0x0303 => "12",
        0x0302 => "11",
        0x0301 => "10",
        0x0300 => "s3",
        else => "00",
    };
}

fn lessU16(_: void, a: u16, b: u16) bool {
    return a < b;
}

/// Writes the twelve first hex digits of the SHA-256 of `list` written as comma-joined 4-digit
/// hex, optionally followed by "_" and `tail` in its own order.
fn hash12(list: []const u16, tail: ?[]const u16, out: *[12]u8) void {
    if (list.len == 0 and (tail == null or tail.?.len == 0)) {
        @memcpy(out, "000000000000");
        return;
    }
    var h = std.crypto.hash.sha2.Sha256.init(.{});
    var b: [5]u8 = undefined;
    for (list, 0..) |v, i| {
        if (i > 0) h.update(",");
        h.update(std.fmt.bufPrint(&b, "{x:0>4}", .{v}) catch unreachable);
    }
    if (tail) |t| if (t.len > 0) {
        h.update("_");
        for (t, 0..) |v, i| {
            if (i > 0) h.update(",");
            h.update(std.fmt.bufPrint(&b, "{x:0>4}", .{v}) catch unreachable);
        }
    };
    var d: [32]u8 = undefined;
    h.final(&d);
    const hex = std.fmt.bytesToHex(d, .lower);
    @memcpy(out, hex[0..12]);
}

fn alpnChars(a: []const u8) [2]u8 {
    if (a.len == 0) return .{ '0', '0' };
    const first = a[0];
    const last = a[a.len - 1];
    if (std.ascii.isAlphanumeric(first) and std.ascii.isAlphanumeric(last)) return .{ first, last };
    // JA4: a protocol that is not alphanumeric gives the first and last hex digit of its bytes.
    const hex = "0123456789abcdef";
    return .{ hex[first >> 4], hex[last & 0x0f] };
}

/// Writes the name into `out` (36 bytes) and returns it.
pub fn ja4(h: Hello, out: *[36]u8) []const u8 {
    var ciphers: [256]u16 = undefined;
    var nc: usize = 0;
    for (h.ciphers) |v| if (!isGrease(v) and nc < ciphers.len) {
        ciphers[nc] = v;
        nc += 1;
    };
    var exts: [128]u16 = undefined;
    var ne_all: usize = 0;
    var ne: usize = 0;
    for (h.extensions) |v| if (!isGrease(v)) {
        ne_all += 1;
        if (v == 0x0000 or v == 0x0010) continue; // SNI and ALPN are counted, not hashed
        if (ne < exts.len) {
            exts[ne] = v;
            ne += 1;
        }
    };
    var sig: [128]u16 = undefined;
    var ns: usize = 0;
    for (h.sigalgs) |v| if (!isGrease(v) and ns < sig.len) {
        sig[ns] = v;
        ns += 1;
    };
    std.mem.sort(u16, ciphers[0..nc], {}, lessU16);
    std.mem.sort(u16, exts[0..ne], {}, lessU16);
    const a = alpnChars(h.alpn);
    var b12: [12]u8 = undefined;
    var c12: [12]u8 = undefined;
    hash12(ciphers[0..nc], null, &b12);
    hash12(exts[0..ne], sig[0..ns], &c12);
    return std.fmt.bufPrint(out, "t{s}{c}{d:0>2}{d:0>2}{c}{c}_{s}_{s}", .{
        versionCode(h.version), @as(u8, if (h.has_sni) 'd' else 'i'), @min(nc, 99), @min(ne_all, 99), a[0], a[1], b12, c12,
    }) catch unreachable;
}

test "grease is left out and order does not matter" {
    var o1: [36]u8 = undefined;
    var o2: [36]u8 = undefined;
    const x = ja4(.{ .version = 0x0304, .has_sni = true, .ciphers = &.{ 0x1301, 0x0a0a, 0x1302 }, .extensions = &.{ 0x0000, 0x002b, 0x1a1a, 0x000d, 0x0010 }, .alpn = "h2", .sigalgs = &.{ 0x0403, 0x0804 } }, &o1);
    const y = ja4(.{ .version = 0x0304, .has_sni = true, .ciphers = &.{ 0x1302, 0x1301 }, .extensions = &.{ 0x000d, 0x0010, 0x002b, 0x0000 }, .alpn = "h2", .sigalgs = &.{ 0x0403, 0x0804 } }, &o2);
    try std.testing.expectEqualStrings(x, y);
    try std.testing.expectEqualStrings("t13d0204h2_", x[0..11]);
}

test "no sni, no alpn, tls 1.2" {
    var o: [36]u8 = undefined;
    const x = ja4(.{ .version = 0x0303, .has_sni = false, .ciphers = &.{0xc02b}, .extensions = &.{0x000a}, .alpn = "", .sigalgs = &.{} }, &o);
    try std.testing.expectEqualStrings("t12i010100_", x[0..11]);
}

test "signature algorithms keep their order" {
    var o1: [36]u8 = undefined;
    var o2: [36]u8 = undefined;
    const x = ja4(.{ .version = 0x0304, .has_sni = true, .ciphers = &.{0x1301}, .extensions = &.{0x000d}, .alpn = "", .sigalgs = &.{ 0x0403, 0x0804 } }, &o1);
    const y = ja4(.{ .version = 0x0304, .has_sni = true, .ciphers = &.{0x1301}, .extensions = &.{0x000d}, .alpn = "", .sigalgs = &.{ 0x0804, 0x0403 } }, &o2);
    try std.testing.expect(!std.mem.eql(u8, x, y));
}
