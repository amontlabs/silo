//! #374/#385 usage.zig: the meter of all four formats, streamed (SSE) or whole, on any bytes.
const std = @import("std");
const s = @import("stream.zig");
const u = @import("usage.zig");
const Sink = struct {
    m: *u.Meter,
    pub fn onEvent(self: *Sink, ev: s.Sse.Event) !void { try self.m.onEvent(ev); }
};
export fn LLVMFuzzerTestOneInput(data: [*]const u8, size: usize) c_int {
    if (size < 1) return 0;
    const gpa = std.heap.c_allocator;
    const fmt: u.Format = @enumFromInt(data[0] % 4);
    const streamed = (data[0] & 4) != 0;
    const input = data[1..size];
    var m = u.Meter.init(gpa, fmt);
    defer m.deinit();
    if (streamed) {
        var ss: s.Sse = .{ .max_event = 1 << 20 };
        defer ss.deinit(gpa);
        var sk: Sink = .{ .m = &m };
        ss.feed(gpa, input, &sk) catch {};
    } else m.onBody(input) catch {};
    _ = m.billed();
    _ = m.pending();
    // Invariants the budget rests on: no count goes negative.
    const b = m.billed();
    if (b.input < 0 or b.output < 0 or b.cache_read < 0 or b.cache_write_short < 0 or b.cache_write_long < 0) @panic("negative count");
    return 0;
}
