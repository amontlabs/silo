//! #374/#385 ws.zig: WebSocket frames, masks, fragments and permessage-deflate, fed in two pieces,
//! each message read by the Responses meter.
const std = @import("std");
const w = @import("ws.zig");
const u = @import("usage.zig");
const Sink = struct {
    m: *u.Meter,
    pub fn onMessage(self: *Sink, msg: w.Message) !void { try self.m.onEvent(.{ .data = msg.data }); }
};
export fn LLVMFuzzerTestOneInput(data: [*]const u8, size: usize) c_int {
    if (size < 2) return 0;
    const gpa = std.heap.c_allocator;
    const flags = data[0];
    const split: usize = data[1];
    const input = data[2..size];
    var m = u.Meter.init(gpa, .responses);
    defer m.deinit();
    var r: w.Reader = .{ .gpa = gpa, .deflate = flags & 1 != 0, .no_context_takeover = flags & 2 != 0, .max_message = 1 << 20 };
    defer r.deinit();
    var sk: Sink = .{ .m = &m };
    // feed() masks in place through a mutable copy: give it a copy, in two pieces.
    const copy = gpa.dupe(u8, input) catch return 0;
    defer gpa.free(copy);
    const k = @min(split, copy.len);
    r.feed(copy[0..k], &sk) catch return 0;
    r.feed(copy[k..], &sk) catch {};
    return 0;
}
