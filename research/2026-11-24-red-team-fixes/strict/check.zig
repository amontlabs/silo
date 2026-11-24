//! Reads files of raw request bytes, as one connection each, and prints what the strict reader makes
//! of every request in them, one JSON line each. The first refusal ends the connection.
//! usage: check <sni> <file>...
const std = @import("std");
const strict = @import("strict.zig");

pub fn main() !void {
    var gpa_state: std.heap.GeneralPurposeAllocator(.{}) = .init;
    const gpa = gpa_state.allocator();
    const args = try std.process.argsAlloc(gpa);
    defer std.process.argsFree(gpa, args);
    var out_buf: [4096]u8 = undefined;
    var w = std.fs.File.stdout().writer(&out_buf);
    const out = &w.interface;
    for (args[2..]) |path| {
        const buf = try std.fs.cwd().readFileAlloc(gpa, path, 256 * 1024 * 1024);
        defer gpa.free(buf);
        var pos: usize = 0;
        var n: usize = 0;
        while (pos < buf.len) {
            n += 1;
            const r = strict.check(gpa, buf[pos..], args[1]) catch |e| switch (e) {
                error.Incomplete => strict.Result{ .verdict = .refused, .why = "the connection ends inside a request" },
                else => return e,
            };
            try out.print("{{\"file\":\"{s}\",\"n\":{d},\"verdict\":\"{s}\",\"why\":\"{s}\",\"method\":\"{s}\",\"body_bytes\":{d},\"decoded_bytes\":{d}}}\n", .{ std.fs.path.basename(path), n, @tagName(r.verdict), r.why, r.method, r.body_bytes, r.decoded_bytes });
            if (r.verdict == .refused) break;
            pos += r.consumed;
        }
    }
    try out.flush();
}
