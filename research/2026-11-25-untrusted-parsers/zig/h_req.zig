//! #374/#385 spend.zig: a model request's body in all four formats, its credential and plan headers.
const std = @import("std");
const sp = @import("spend.zig");
const u = @import("usage.zig");
export fn LLVMFuzzerTestOneInput(data: [*]const u8, size: usize) c_int {
    if (size < 1) return 0;
    const gpa = std.heap.c_allocator;
    const fmt: u.Format = @enumFromInt(data[0] % 4);
    const input = data[1..size];
    // path | body, split at the first NUL
    const nul = std.mem.indexOfScalar(u8, input, 0) orelse 0;
    const path = input[0..nul];
    const body = input[@min(nul + 1, input.len)..];
    if (sp.readRequest(gpa, fmt, path, body)) |r_| {
        var r = r_;
        r.deinit(gpa);
    } else |_| {}
    _ = sp.credentialOf(body, path);
    _ = sp.planWindows(body);
    return 0;
}
