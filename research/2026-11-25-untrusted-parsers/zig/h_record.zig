//! #386 verify, fed lines that pass the checks a fuzzer cannot forge: the harness writes the
//! segment header, each line's envelope prefix (seq and prev), and each frame's CRC, so the
//! fuzzer's bytes reach the JSON reader and every check after it. Input: lines split by '\n';
//! the first byte picks the options.
const std = @import("std");
const format = @import("format.zig");
const verify = @import("verify.zig");

const AllBlobs = struct {
    fn check(_: *anyopaque, hex: []const u8) verify.BlobState {
        return switch (hex[0]) { '0'...'4' => .whole, '5'...'9' => .missing, else => .changed };
    }
};
var dummy: u8 = 0;

export fn LLVMFuzzerTestOneInput(data: [*]const u8, size: usize) c_int {
    if (size < 1) return 0;
    const gpa = std.heap.c_allocator;
    const flags = data[0];
    const input = data[1..size];
    var seg: std.ArrayList(u8) = .empty;
    defer seg.deinit(gpa);
    const hdr = (format.Header{ .computer = 1, .segment = 1, .first_seq = 1, .prev = format.zero_hash }).encode();
    seg.appendSlice(gpa, &hdr) catch return 0;
    var prev = format.zero_hash;
    var seq: u64 = 1;
    var it = std.mem.splitScalar(u8, input, '\n');
    var nlines: u64 = 0;
    var last_hash = format.zero_hash;
    while (it.next()) |rest| : (seq += 1) {
        var payload: std.ArrayList(u8) = .empty;
        defer payload.deinit(gpa);
        payload.print(gpa, "{{\"v\":1,\"seq\":{d},\"prev\":\"{s}\",\"time\":\"", .{ seq, format.hex(prev) }) catch return 0;
        payload.appendSlice(gpa, rest) catch return 0;
        if (payload.items.len > format.max_line) break;
        seg.appendSlice(gpa, &format.frameHeader(payload.items)) catch return 0;
        seg.appendSlice(gpa, payload.items) catch return 0;
        prev = format.lineHash(payload.items);
        last_hash = prev;
        nlines += 1;
    }
    const segs = [_][]const u8{seg.items};
    const blobs: verify.Blobs = .{ .ctx = &dummy, .check = AllBlobs.check };
    const r = verify.verify(&segs, .{
        .computer = "c1",
        .chain = if (flags & 1 != 0) .opinions else .record,
        .blobs = if (flags & 2 != 0) blobs else null,
        .head = if (flags & 4 != 0) .{ .seq = nlines, .hash = last_hash } else null,
    });
    // A record whose every line held can not be reported as holding fewer lines.
    if (r.ok() and r.lines != nlines) @panic("verify said the chain holds but counted other lines");
    return 0;
}
