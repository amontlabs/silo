//! Makes the placeholder a computer holds for a secret: one per secret and per computer, random, and
//! shaped like the value, so that a program that checks a credential's form takes it.
//!
//! - A JWT keeps its header and claims, which programs decode (#379: Azure's tokens), and gets a
//!   random signature of the same length.
//! - Anything else keeps a known prefix (`sk-ant-api03-`, `ghp_`, `AIza`...), its length, and the
//!   class of each character: a digit stays a digit, a letter a letter of the same case, and any
//!   other character stays as it is (the dashes of `xoxb-1-2-abc`).
//!
//! A value with fewer than 96 random bits left after that is refused: its placeholder could be
//! guessed, or typed by chance.
const std = @import("std");

const prefixes = [_][]const u8{
    "sk-ant-api03-", "sk-ant-oat01-", "sk-ant-",  "sk-proj-",  "sk-svcacct-", "sk-admin-", "sk-",
    "github_pat_",   "ghp_",          "gho_",     "ghu_",      "ghs_",        "ghr_",      "glpat-",
    "xoxb-",         "xoxp-",         "xapp-",    "AIza",      "ya29.",       "1//",       "hf_",
    "npm_",          "pypi-",         "sk_live_", "sk_test_",  "rk_live_",    "dop_v1_",   "FlyV1 ",
};

pub const Error = error{ TooShort, OutOfMemory };

const lower = "abcdefghijklmnopqrstuvwxyz";
const upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const digits = "0123456789";
const b64url = upper ++ lower ++ digits ++ "-_";

fn isJwt(v: []const u8) bool {
    if (!std.mem.startsWith(u8, v, "eyJ")) return false;
    var it = std.mem.splitScalar(u8, v, '.');
    var n: usize = 0;
    while (it.next()) |part| : (n += 1) {
        if (part.len == 0) return false;
        for (part) |ch| if (std.mem.indexOfScalar(u8, b64url, ch) == null) return false;
    }
    return n == 3;
}

pub fn make(gpa: std.mem.Allocator, value: []const u8, rng: std.Random) Error![]u8 {
    const out = try gpa.dupe(u8, value);
    errdefer gpa.free(out);
    var bits: f64 = 0;
    var from: usize = 0;
    if (isJwt(value)) {
        from = std.mem.lastIndexOfScalar(u8, value, '.').? + 1;
        for (out[from..]) |*ch| ch.* = b64url[rng.uintLessThan(usize, b64url.len)];
        bits = @as(f64, @floatFromInt(out.len - from)) * 6;
    } else {
        for (prefixes) |p| if (std.mem.startsWith(u8, value, p)) {
            from = p.len;
            break;
        };
        for (out[from..]) |*ch| {
            const set: []const u8 = if (std.ascii.isDigit(ch.*)) digits else if (std.ascii.isLower(ch.*)) lower else if (std.ascii.isUpper(ch.*)) upper else continue;
            ch.* = set[rng.uintLessThan(usize, set.len)];
            bits += std.math.log2(@as(f64, @floatFromInt(set.len)));
        }
    }
    if (bits < 96) return error.TooShort;
    // A placeholder equal to its value has odds of 2^-96; still, never hand one out.
    if (std.mem.eql(u8, out, value)) return make(gpa, value, rng);
    return out;
}

const t = std.testing;

fn sameShape(a: []const u8, b: []const u8, from: usize) !void {
    try t.expectEqual(a.len, b.len);
    try t.expectEqualStrings(a[0..from], b[0..from]);
    for (a[from..], b[from..]) |x, y| {
        try t.expectEqual(std.ascii.isDigit(x), std.ascii.isDigit(y));
        try t.expectEqual(std.ascii.isLower(x), std.ascii.isLower(y));
        try t.expectEqual(std.ascii.isUpper(x), std.ascii.isUpper(y));
        if (!std.ascii.isAlphanumeric(x)) try t.expectEqual(x, y);
    }
}

test "shaped like the value, random, one each time" {
    var prng = std.Random.DefaultPrng.init(7);
    const r = prng.random();
    const anth = "sk-ant-api03-Ab3dEf6hIj9kLm2nOp5qRs8tUv1wXy4zAb7cDe0fGh3iJk6lMn9oPq2rSt5uVw8xYz1aBc4dEf7gHi0jKl3mNo6pQr9sTu2vWx5yZa8bCd1eFg4hIj7k-AbCdEf12";
    const a = try make(t.allocator, anth, r);
    defer t.allocator.free(a);
    try sameShape(anth, a, "sk-ant-api03-".len);
    const b = try make(t.allocator, anth, r);
    defer t.allocator.free(b);
    try t.expect(!std.mem.eql(u8, a, b));

    // A made-up Slack bot token, split so that secret scanners do not take this line for one.
    const slack = "xox" ++ "b-123456789012-1234567890123-AbCdEfGhIjKlMnOpQrStUvWx";
    const s = try make(t.allocator, slack, r);
    defer t.allocator.free(s);
    try sameShape(slack, s, 5);

    const jwt = "eyJ0eXAiOiJKV1QiLCJhbGciOiJSUzI1NiJ9.eyJhdWQiOiJodHRwczovL21hbmFnZW1lbnQuYXp1cmUuY29tLyJ9.c2lnbmF0dXJlLXRoYXQtaXMtbG9uZy1lbm91Z2gtZm9yLTk2LWJpdHM";
    const j = try make(t.allocator, jwt, r);
    defer t.allocator.free(j);
    const dot = std.mem.lastIndexOfScalar(u8, jwt, '.').? + 1;
    try t.expectEqualStrings(jwt[0..dot], j[0..dot]);
    try t.expectEqual(jwt.len, j.len);
    try t.expect(!std.mem.eql(u8, jwt[dot..], j[dot..]));

    try t.expectError(error.TooShort, make(t.allocator, "sk-ant-api03-short", r));
    try t.expectError(error.TooShort, make(t.allocator, "123456", r));
}
