//! #374/#385 stream.zig: HTTP chunked framing, server-sent events, the tool-call watcher of both
//! providers and the tool-result reader of requests, on any bytes.
const std = @import("std");
const s = @import("stream.zig");
const u = @import("usage.zig");
export fn LLVMFuzzerTestOneInput(data: [*]const u8, size: usize) c_int {
    const input = data[0..size];
    const gpa = std.heap.c_allocator;
    var c: s.Chunked = .{};
    var out: std.ArrayList(u8) = .empty;
    defer out.deinit(gpa);
    c.feed(gpa, input, &out) catch {};
    inline for (.{ s.Provider.anthropic, s.Provider.openai }) |p| {
        var w = s.Watcher.init(gpa, p);
        defer w.deinit();
        var ss: s.Sse = .{};
        defer ss.deinit(gpa);
        ss.feed(gpa, input, &w) catch {};
        if (s.toolResultIds(gpa, p, input)) |ids_| {
            var ids = ids_;
            for (ids.items) |x| gpa.free(x);
            ids.deinit(gpa);
        } else |_| {}
    }
    return 0;
}
