//! #374/#385 ws.zig: a permessage-deflate message whose last DEFLATE block has BFINAL set.
//! RFC 7692 lets a sender end a message that way. zlib then answers Z_STREAM_END with input left
//! (the 4-byte tail the reader appends), and the loop in `inflate` never ends.
//! Run: zig test -lc -lz -I. ws_final_block.zig  (with ws.zig of #385 beside it), under `timeout`.
const std = @import("std");
const ws = @import("ws.zig");

const Got = struct {
    n: usize = 0,
    pub fn onMessage(self: *Got, m: ws.Message) !void {
        _ = m;
        self.n += 1;
    }
};

test "a deflated message that ends in a final block is read, once" {
    // python: c=zlib.compressobj(6,zlib.DEFLATED,-15); c.compress(b'{"type":"response.create"}')+c.flush(zlib.Z_FINISH)
    const d = [_]u8{ 0xab, 0x56, 0x2a, 0xa9, 0x2c, 0x48, 0x55, 0xb2, 0x52, 0x2a, 0x4a, 0x2d, 0x2e, 0xc8, 0xcf, 0x2b, 0x4e, 0xd5, 0x4b, 0x2e, 0x4a, 0x4d, 0x2c, 0x49, 0x55, 0xaa, 0x05, 0x00 };
    var frame = [_]u8{ 0xc1, d.len } ++ d; // FIN, RSV1 (compressed), text
    var r: ws.Reader = .{ .gpa = std.testing.allocator, .deflate = true };
    defer r.deinit();
    var got: Got = .{};
    try r.feed(&frame, &got);
    try std.testing.expectEqual(@as(usize, 1), got.n);
}
