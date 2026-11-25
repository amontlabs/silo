//! A strict reader of HTTP/1.1 requests and of the bodies the boundary rules on.
//!
//! The rule it implements: a request to a host the boundary reads leaves only if the boundary read
//! all of it the way any conforming host would. Anything on which two conforming parsers could
//! disagree is refused, before a byte of it is forwarded. Nothing is ever rewritten: a request is
//! forwarded as it came, or refused (`forwarded-byte-for-byte` keeps its promise).
//!
//! Three outcomes:
//!   .read     the whole request was read: head, body, and the body's content when it is JSON or a form
//!   .unread   a well-framed request whose body is of a type the boundary does not parse
//!             (an upload, say). Framing is still certain, so it may pass where no rule needs the body.
//!   .refused  with the reason: framing or content that a host could read another way.
const std = @import("std");
const c = @cImport({
    @cInclude("zlib.h");
    @cInclude("zstd.h");
});

pub const max_head = 64 * 1024;
pub const max_headers = 100;
pub const max_body = 64 * 1024 * 1024;
pub const max_depth = 128;

pub const Verdict = enum { read, unread, refused };

pub const Result = struct {
    verdict: Verdict,
    why: []const u8 = "",
    /// Bytes of the input this request took: the next request starts there.
    consumed: usize = 0,
    method: []const u8 = "",
    target: []const u8 = "",
    body_bytes: usize = 0,
    decoded_bytes: usize = 0,
};

pub const Incomplete = error{Incomplete};

fn refuse(why: []const u8) Result {
    return .{ .verdict = .refused, .why = why };
}

fn isTchar(b: u8) bool {
    return std.ascii.isAlphanumeric(b) or std.mem.indexOfScalar(u8, "!#$%&'*+-.^_`|~", b) != null;
}

/// Headers that a request may carry once only: a second copy is a second meaning.
const singletons = [_][]const u8{
    "host",          "content-length", "transfer-encoding", "content-encoding", "content-type",
    "authorization", "x-api-key",      "upgrade",           "expect",           "cookie",
};

/// Reads one request from the start of `buf`. `sni` is the name in the TLS handshake.
/// Returns error.Incomplete when `buf` ends before the request does.
pub fn check(gpa: std.mem.Allocator, buf: []const u8, sni: []const u8) !Result {
    // ---- the head
    const end = std.mem.indexOf(u8, buf, "\r\n\r\n") orelse {
        if (buf.len > max_head) return refuse("a head longer than 64 KiB");
        // A bare LF ending the head would never be found: look for it now, so it is refused, not waited on.
        if (std.mem.indexOf(u8, buf, "\n\n") != null or std.mem.indexOf(u8, buf, "\r\n\n") != null) return refuse("a line that ends in a bare LF");
        return error.Incomplete;
    };
    if (end > max_head) return refuse("a head longer than 64 KiB");
    const head = buf[0..end];
    for (head, 0..) |b, i| {
        if (b == '\n' and (i == 0 or head[i - 1] != '\r')) return refuse("a line that ends in a bare LF");
        if (b == '\r' and (i + 1 >= head.len or head[i + 1] != '\n')) return refuse("a bare CR in the head");
        if ((b < 0x20 and b != '\t' and b != '\r' and b != '\n') or b == 0x7f) return refuse("a control character in the head");
    }

    var lines = std.mem.splitSequence(u8, head, "\r\n");
    const first = lines.next().?;
    // METHOD SP origin-form SP HTTP/1.1, each part exactly once.
    var parts = std.mem.splitScalar(u8, first, ' ');
    const method = parts.next() orelse return refuse("an empty request line");
    const target = parts.next() orelse return refuse("a request line without a target");
    const version = parts.next() orelse return refuse("a request line without a version");
    if (parts.next() != null) return refuse("a request line with more than three parts");
    if (method.len == 0) return refuse("an empty method");
    for (method) |b| if (!std.ascii.isUpper(b)) return refuse("a method that is not an upper-case token");
    if (!std.mem.eql(u8, version, "HTTP/1.1")) return refuse("a version other than HTTP/1.1");
    if (target.len == 0 or target[0] != '/') return refuse("a request target not in origin form (absolute, authority or asterisk)");
    if (target.len > 1 and target[1] == '/') return refuse("a request target that starts with two slashes");
    for (target) |b| if (b <= 0x20 or b >= 0x7f) return refuse("a request target with a character outside visible ASCII");

    var host: ?[]const u8 = null;
    var content_length: ?u64 = null;
    var chunked = false;
    var encoding: []const u8 = "identity";
    var ctype: []const u8 = "";
    var counts = [_]u8{0} ** singletons.len;
    var n: usize = 0;
    while (lines.next()) |line| {
        n += 1;
        if (n > max_headers) return refuse("more than 100 header lines");
        if (line.len == 0) return refuse("an empty header line");
        if (line[0] == ' ' or line[0] == '\t') return refuse("a folded header line (obs-fold)");
        const colon = std.mem.indexOfScalar(u8, line, ':') orelse return refuse("a header line without a colon");
        const name = line[0..colon];
        if (name.len == 0) return refuse("an empty header name");
        for (name) |b| if (!isTchar(b)) return refuse("a header name with a character outside the token set (whitespace before the colon, say)");
        const value = std.mem.trim(u8, line[colon + 1 ..], " \t");
        for (singletons, 0..) |s, i| if (std.ascii.eqlIgnoreCase(name, s)) {
            counts[i] += 1;
            if (counts[i] > 1) return refuse("a header that may appear once appears twice");
        };
        if (std.ascii.eqlIgnoreCase(name, "host")) host = value;
        if (std.ascii.eqlIgnoreCase(name, "content-length")) {
            if (value.len == 0 or value.len > 12) return refuse("a Content-Length that is not 1 to 12 digits");
            for (value) |b| if (!std.ascii.isDigit(b)) return refuse("a Content-Length with something other than digits (a sign, a list, a space)");
            content_length = std.fmt.parseInt(u64, value, 10) catch unreachable;
            if (content_length.? > max_body) return refuse("a body larger than 64 MiB");
        }
        if (std.ascii.eqlIgnoreCase(name, "transfer-encoding")) {
            if (!std.ascii.eqlIgnoreCase(value, "chunked")) return refuse("a Transfer-Encoding other than one plain chunked");
            chunked = true;
        }
        if (std.ascii.eqlIgnoreCase(name, "content-encoding")) encoding = value;
        if (std.ascii.eqlIgnoreCase(name, "content-type")) ctype = value;
        if (std.ascii.eqlIgnoreCase(name, "upgrade") and !std.ascii.eqlIgnoreCase(value, "websocket")) return refuse("an Upgrade to something other than a WebSocket (h2c, say)");
        if (std.ascii.eqlIgnoreCase(name, "http2-settings")) return refuse("an HTTP2-Settings header");
        if (std.ascii.eqlIgnoreCase(name, "trailer")) return refuse("a Trailer header");
    }
    if (chunked and content_length != null) return refuse("both Content-Length and Transfer-Encoding");
    const h = host orelse return refuse("no Host header");
    var hname = h;
    if (std.mem.endsWith(u8, hname, ":443")) hname = hname[0 .. hname.len - 4];
    if (!std.ascii.eqlIgnoreCase(hname, sni)) return refuse("a Host that is not the name in the TLS handshake");

    // ---- the body, framed
    var pos = end + 4;
    var body: std.ArrayList(u8) = .empty;
    defer body.deinit(gpa);
    if (chunked) {
        while (true) {
            const le = std.mem.indexOfPos(u8, buf, pos, "\r\n") orelse {
                if (buf.len - pos > 32) return refuse("a chunk size line longer than 16 hex digits, or with an extension");
                return error.Incomplete;
            };
            const size_line = buf[pos..le];
            if (size_line.len == 0 or size_line.len > 15) return refuse("a chunk size that is empty, longer than 15 hex digits, or carries an extension");
            for (size_line) |b| if (!std.ascii.isHex(b)) return refuse("a chunk size with something other than hex digits (an extension, a space, a sign, a 0x)");
            const size = std.fmt.parseInt(u64, size_line, 16) catch unreachable;
            pos = le + 2;
            if (size == 0) {
                if (buf.len < pos + 2) return error.Incomplete;
                if (!std.mem.eql(u8, buf[pos .. pos + 2], "\r\n")) return refuse("a trailer field after the last chunk");
                pos += 2;
                break;
            }
            if (body.items.len + size > max_body) return refuse("a body larger than 64 MiB");
            if (buf.len < pos + size + 2) return error.Incomplete;
            try body.appendSlice(gpa, buf[pos .. pos + size]);
            if (!std.mem.eql(u8, buf[pos + size .. pos + size + 2], "\r\n")) return refuse("chunk data not followed by CRLF");
            pos += size + 2;
        }
    } else if (content_length) |cl| {
        if (buf.len < pos + cl) return error.Incomplete;
        try body.appendSlice(gpa, buf[pos .. pos + cl]);
        pos += cl;
    }
    var r: Result = .{ .verdict = .read, .consumed = pos, .method = method, .target = target, .body_bytes = body.items.len };
    if (body.items.len == 0) return r;

    // ---- the body, decoded whole
    var decoded: std.ArrayList(u8) = .empty;
    defer decoded.deinit(gpa);
    if (std.ascii.eqlIgnoreCase(encoding, "identity")) {
        try decoded.appendSlice(gpa, body.items);
    } else if (std.ascii.eqlIgnoreCase(encoding, "gzip") or std.ascii.eqlIgnoreCase(encoding, "deflate")) {
        if (inflate(gpa, body.items, if (std.ascii.eqlIgnoreCase(encoding, "gzip")) 31 else 15, &decoded)) |why| return refuse(why);
    } else if (std.ascii.eqlIgnoreCase(encoding, "zstd")) {
        if (unzstd(gpa, body.items, &decoded)) |why| return refuse(why);
    } else {
        return refuse("a body encoding the boundary cannot decode (br, a list, an unknown one)");
    }
    r.decoded_bytes = decoded.items.len;

    // ---- the content
    const mt = std.mem.trim(u8, if (std.mem.indexOfScalar(u8, ctype, ';')) |i| ctype[0..i] else ctype, " ");
    if (std.ascii.eqlIgnoreCase(mt, "application/json") or (mt.len == 0 and decoded.items.len > 0 and (decoded.items[0] == '{' or decoded.items[0] == '['))) {
        if (try strictJson(gpa, decoded.items)) |why| return refuse(why);
        return r;
    }
    if (std.ascii.eqlIgnoreCase(mt, "application/x-www-form-urlencoded")) {
        if (try strictForm(gpa, decoded.items)) |why| return refuse(why);
        return r;
    }
    r.verdict = .unread;
    r.why = "a body of a type the boundary does not parse";
    return r;
}

fn inflate(gpa: std.mem.Allocator, in: []const u8, window: c_int, out: *std.ArrayList(u8)) ?[]const u8 {
    var z: c.z_stream = std.mem.zeroes(c.z_stream);
    if (c.inflateInit2(&z, window) != c.Z_OK) return "the decoder would not start";
    defer _ = c.inflateEnd(&z);
    z.next_in = @constCast(in.ptr);
    z.avail_in = @intCast(in.len);
    var chunk: [65536]u8 = undefined;
    while (true) {
        z.next_out = &chunk;
        z.avail_out = chunk.len;
        const rc = c.inflate(&z, c.Z_NO_FLUSH);
        out.appendSlice(gpa, chunk[0 .. chunk.len - z.avail_out]) catch return "out of memory";
        if (out.items.len > max_body) return "a decoded body larger than 64 MiB";
        if (rc == c.Z_STREAM_END) break;
        if (rc != c.Z_OK) return "a compressed body that does not decode";
        if (z.avail_in == 0 and z.avail_out != 0) return "a compressed body that ends early";
    }
    // gzip allows a second member, and decoders differ on reading it: refuse anything after the first.
    if (z.avail_in != 0) return "bytes after the end of the compressed body (a second gzip member, say)";
    return null;
}

fn unzstd(gpa: std.mem.Allocator, in: []const u8, out: *std.ArrayList(u8)) ?[]const u8 {
    const frame = c.ZSTD_findFrameCompressedSize(in.ptr, in.len);
    if (c.ZSTD_isError(frame) != 0) return "a zstd body that does not decode";
    if (frame != in.len) return "bytes after the first zstd frame (a second frame, say)";
    const d = c.ZSTD_createDStream() orelse return "the decoder would not start";
    defer _ = c.ZSTD_freeDStream(d);
    var ib: c.ZSTD_inBuffer = .{ .src = in.ptr, .size = in.len, .pos = 0 };
    var chunk: [65536]u8 = undefined;
    while (true) {
        var ob: c.ZSTD_outBuffer = .{ .dst = &chunk, .size = chunk.len, .pos = 0 };
        const rc = c.ZSTD_decompressStream(d, &ob, &ib);
        if (c.ZSTD_isError(rc) != 0) return "a zstd body that does not decode";
        out.appendSlice(gpa, chunk[0..ob.pos]) catch return "out of memory";
        if (out.items.len > max_body) return "a decoded body larger than 64 MiB";
        if (rc == 0) break;
        if (ib.pos == ib.size and ob.pos < ob.size) return "a zstd body that ends early";
    }
    return null;
}

/// RFC 8259 and nothing more, plus what makes two parsers read one document two ways:
/// duplicate keys, keys equal but for case (Go matches fields that way), lone surrogates,
/// integers past 2^53, numbers with more than 40 characters, nesting past 128.
pub fn strictJson(gpa: std.mem.Allocator, doc: []const u8) !?[]const u8 {
    if (!std.unicode.utf8ValidateSlice(doc)) return "a JSON body that is not valid UTF-8";
    if (std.mem.startsWith(u8, doc, "\xEF\xBB\xBF")) return "a JSON body that starts with a byte order mark";
    if (loneSurrogate(doc)) return "a JSON string with a lone surrogate escape";
    var arena = std.heap.ArenaAllocator.init(gpa);
    defer arena.deinit();
    const a = arena.allocator();
    var sc = std.json.Scanner.initCompleteInput(a, doc);
    const Frame = struct { object: bool, want_key: bool, keys: std.ArrayList([]const u8) };
    var stack: std.ArrayList(Frame) = .empty;
    while (true) {
        const tok = sc.nextAlloc(a, .alloc_always) catch return "a body that does not parse as JSON (comments, a trailing comma, NaN, trailing bytes...)";
        const top: ?*Frame = if (stack.items.len > 0) &stack.items[stack.items.len - 1] else null;
        switch (tok) {
            .object_begin, .array_begin => {
                if (top) |t| if (t.object) {
                    t.want_key = true;
                };
                if (stack.items.len >= max_depth) return "JSON nested deeper than 128";
                try stack.append(a, .{ .object = tok == .object_begin, .want_key = tok == .object_begin, .keys = .empty });
            },
            .object_end, .array_end => {
                _ = stack.pop();
                continue;
            },
            .allocated_string => |s| {
                if (top) |t| if (t.object and t.want_key) {
                    for (t.keys.items) |k| {
                        if (std.mem.eql(u8, k, s)) return "a JSON object with the same key twice";
                        if (std.ascii.eqlIgnoreCase(k, s)) return "a JSON object with two keys equal but for case";
                    }
                    try t.keys.append(a, s);
                    t.want_key = false;
                    continue;
                };
                if (top) |t| if (t.object) {
                    t.want_key = true;
                };
            },
            .allocated_number => |num| {
                if (num.len > 40) return "a JSON number longer than 40 characters";
                if (std.mem.indexOfAny(u8, num, ".eE") == null) {
                    const v = std.fmt.parseInt(i128, num, 10) catch return "a JSON integer out of range";
                    if (v > (1 << 53) or v < -(1 << 53)) return "a JSON integer past 2^53, read differently by doubles and integers";
                } else {
                    const f = std.fmt.parseFloat(f64, num) catch return "a JSON number that does not parse";
                    if (std.math.isInf(f)) return "a JSON number past the largest double (infinity for some parsers, an error for others)";
                }
                if (top) |t| if (t.object) {
                    t.want_key = true;
                };
            },
            .true, .false, .null => if (top) |t| if (t.object) {
                t.want_key = true;
            },
            .end_of_document => return null,
            else => {},
        }
    }
}

fn loneSurrogate(doc: []const u8) bool {
    var i: usize = 0;
    while (i + 6 <= doc.len) : (i += 1) {
        if (doc[i] != '\\') continue;
        if (doc[i + 1] != 'u') {
            i += 1; // skip the escaped character, so `\\u` is not read as an escape
            continue;
        }
        const v = std.fmt.parseInt(u16, doc[i + 2 .. i + 6], 16) catch return false;
        if (v >= 0xD800 and v <= 0xDBFF) {
            if (i + 12 > doc.len or doc[i + 6] != '\\' or doc[i + 7] != 'u') return true;
            const w = std.fmt.parseInt(u16, doc[i + 8 .. i + 12], 16) catch return true;
            if (w < 0xDC00 or w > 0xDFFF) return true;
            i += 11;
        } else if (v >= 0xDC00 and v <= 0xDFFF) return true else i += 5;
    }
    return false;
}

/// `k=v&k=v`: valid percent escapes, no key twice (decoded, case kept), no empty pair.
fn strictForm(gpa: std.mem.Allocator, body: []const u8) !?[]const u8 {
    var arena = std.heap.ArenaAllocator.init(gpa);
    defer arena.deinit();
    var seen: std.ArrayList([]const u8) = .empty;
    var it = std.mem.splitScalar(u8, body, '&');
    while (it.next()) |pair| {
        if (pair.len == 0) return "a form with an empty pair";
        const eq = std.mem.indexOfScalar(u8, pair, '=') orelse return "a form pair without =";
        const key = pair[0..eq];
        for (pair, 0..) |b, i| {
            if (b == '%') {
                if (i + 2 >= pair.len or !std.ascii.isHex(pair[i + 1]) or !std.ascii.isHex(pair[i + 2])) return "a form with a bad percent escape";
            } else if (b <= 0x20 or b >= 0x7f or b == ';') return "a form with a character that must be escaped";
        }
        for (seen.items) |k| if (std.mem.eql(u8, k, key)) return "a form with the same key twice";
        try seen.append(arena.allocator(), key);
    }
    return null;
}

test "an ordinary request is read" {
    const r = try check(std.testing.allocator, "POST /v1/messages HTTP/1.1\r\nHost: api.anthropic.com\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}", "api.anthropic.com");
    try std.testing.expectEqual(Verdict.read, r.verdict);
}
