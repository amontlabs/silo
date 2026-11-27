//! Reads a copy of a WebSocket stream (RFC 6455), one direction: frames, masks, fragmented
//! messages, and permessage-deflate (RFC 7692) with context takeover. The bytes forwarded are the
//! ones that came; this only reads.
const std = @import("std");
const Allocator = std.mem.Allocator;
const z = @cImport(@cInclude("zlib.h"));

pub const Message = struct { opcode: u4, data: []const u8 };

pub const Reader = struct {
    gpa: Allocator,
    buf: std.ArrayList(u8) = .empty,
    msg: std.ArrayList(u8) = .empty,
    inflated: std.ArrayList(u8) = .empty,
    msg_opcode: u4 = 0,
    msg_compressed: bool = false,
    deflate: bool = false,
    no_context_takeover: bool = false,
    strm: z.z_stream = std.mem.zeroes(z.z_stream),
    strm_ready: bool = false,
    max_message: usize = 64 << 20,
    closed: bool = false,

    pub fn deinit(self: *Reader) void {
        self.buf.deinit(self.gpa);
        self.msg.deinit(self.gpa);
        self.inflated.deinit(self.gpa);
        if (self.strm_ready) _ = z.inflateEnd(&self.strm);
    }

    /// Calls `sink.onMessage(Message)` for each whole data message.
    pub fn feed(self: *Reader, in: []const u8, sink: anytype) !void {
        try self.buf.appendSlice(self.gpa, in);
        var off: usize = 0;
        defer {
            const rest = self.buf.items.len - off;
            std.mem.copyForwards(u8, self.buf.items[0..rest], self.buf.items[off..]);
            self.buf.shrinkRetainingCapacity(rest);
        }
        while (true) {
            const b = self.buf.items[off..];
            if (b.len < 2) return;
            const fin = b[0] & 0x80 != 0;
            const rsv1 = b[0] & 0x40 != 0;
            const opcode: u4 = @truncate(b[0] & 0x0f);
            const masked = b[1] & 0x80 != 0;
            var len: u64 = b[1] & 0x7f;
            var h: usize = 2;
            if (len == 126) {
                if (b.len < 4) return;
                len = std.mem.readInt(u16, b[2..4], .big);
                h = 4;
            } else if (len == 127) {
                if (b.len < 10) return;
                len = std.mem.readInt(u64, b[2..10], .big);
                h = 10;
            }
            if (len > self.max_message) return error.FrameTooLarge;
            const mask_at = h;
            if (masked) h += 4;
            if (b.len < h + len) return;
            const payload = b[h .. h + @as(usize, @intCast(len))];
            if (masked) {
                const m = b[mask_at .. mask_at + 4];
                for (payload, 0..) |*p, i| p.* ^= m[i % 4];
            }
            off += h + @as(usize, @intCast(len));
            if (opcode >= 8) {
                if (opcode == 8) self.closed = true;
                continue; // control frames carry no message
            }
            if (opcode != 0) {
                self.msg.clearRetainingCapacity();
                self.msg_opcode = opcode;
                self.msg_compressed = rsv1 and self.deflate;
            }
            if (self.msg.items.len + payload.len > self.max_message) return error.MessageTooLarge;
            try self.msg.appendSlice(self.gpa, payload);
            if (!fin) continue;
            if (self.msg_compressed) {
                try self.inflate(self.msg.items);
                try sink.onMessage(.{ .opcode = self.msg_opcode, .data = self.inflated.items });
            } else try sink.onMessage(.{ .opcode = self.msg_opcode, .data = self.msg.items });
            self.msg.clearRetainingCapacity();
        }
    }

    fn inflate(self: *Reader, data: []const u8) !void {
        if (!self.strm_ready) {
            if (z.inflateInit2_(&self.strm, -15, z.zlibVersion(), @sizeOf(z.z_stream)) != z.Z_OK) return error.Inflate;
            self.strm_ready = true;
        }
        self.inflated.clearRetainingCapacity();
        const tail = [_]u8{ 0, 0, 0xff, 0xff };
        var tmp: [65536]u8 = undefined;
        for ([_][]const u8{ data, &tail }) |part| {
            self.strm.next_in = @constCast(part.ptr);
            self.strm.avail_in = @intCast(part.len);
            while (self.strm.avail_in > 0) {
                self.strm.next_out = &tmp;
                self.strm.avail_out = tmp.len;
                const r = z.inflate(&self.strm, z.Z_SYNC_FLUSH);
                if (r != z.Z_OK and r != z.Z_BUF_ERROR and r != z.Z_STREAM_END) return error.Inflate;
                try self.inflated.appendSlice(self.gpa, tmp[0 .. tmp.len - self.strm.avail_out]);
                if (self.inflated.items.len > self.max_message) return error.MessageTooLarge;
                if (r == z.Z_BUF_ERROR) break;
                // A final block ends the stream: what follows it (the tail) is not read. The next
                // message starts a new stream.
                if (r == z.Z_STREAM_END) {
                    _ = z.inflateReset(&self.strm);
                    break;
                }
            }
        }
        if (self.no_context_takeover) _ = z.inflateReset(&self.strm);
    }
};

const Got = struct {
    gpa: Allocator,
    msgs: std.ArrayList([]u8) = .empty,
    fn onMessage(self: *Got, m: Message) !void {
        try self.msgs.append(self.gpa, try self.gpa.dupe(u8, m.data));
    }
    fn deinit(self: *Got) void {
        for (self.msgs.items) |x| self.gpa.free(x);
        self.msgs.deinit(self.gpa);
    }
};

test "masked, fragmented, with a ping between, split at every byte" {
    const gpa = std.testing.allocator;
    // "Hel" (masked, not fin) + ping + "lo" (continuation, fin)
    const mask = [_]u8{ 1, 2, 3, 4 };
    var raw: std.ArrayList(u8) = .empty;
    defer raw.deinit(gpa);
    try raw.appendSlice(gpa, &[_]u8{ 0x01, 0x83 });
    try raw.appendSlice(gpa, &mask);
    for ("Hel", 0..) |ch, i| try raw.append(gpa, ch ^ mask[i % 4]);
    try raw.appendSlice(gpa, &[_]u8{ 0x89, 0x00 });
    try raw.appendSlice(gpa, &[_]u8{ 0x80, 0x02, 'l', 'o' });
    var k: usize = 0;
    while (k <= raw.items.len) : (k += 1) {
        var r: Reader = .{ .gpa = gpa };
        defer r.deinit();
        var got: Got = .{ .gpa = gpa };
        defer got.deinit();
        const copy = try gpa.dupe(u8, raw.items);
        defer gpa.free(copy);
        try r.feed(copy[0..k], &got);
        try r.feed(copy[k..], &got);
        try std.testing.expectEqual(@as(usize, 1), got.msgs.items.len);
        try std.testing.expectEqualStrings("Hello", got.msgs.items[0]);
    }
}

test "permessage-deflate with context takeover" {
    const gpa = std.testing.allocator;
    // Two messages compressed with one raw deflate stream, sync-flushed, tails removed:
    // python: c=zlib.compressobj(6,zlib.DEFLATED,-15); [c.compress(m)+c.flush(zlib.Z_SYNC_FLUSH)][:-4]
    const m1 = [_]u8{ 0xf2, 0x48, 0xcd, 0xc9, 0xc9, 0x07, 0x00 };
    const m2 = [_]u8{ 0xf2, 0x00, 0x11, 0x00, 0x00 };
    var r: Reader = .{ .gpa = gpa, .deflate = true };
    defer r.deinit();
    var got: Got = .{ .gpa = gpa };
    defer got.deinit();
    var f1 = [_]u8{ 0xc1, m1.len } ++ m1;
    var f2 = [_]u8{ 0xc1, m2.len } ++ m2;
    try r.feed(&f1, &got);
    try r.feed(&f2, &got);
    try std.testing.expectEqualStrings("Hello", got.msgs.items[0]);
    try std.testing.expectEqualStrings("Hello", got.msgs.items[1]);
}

test "fuzz the websocket reader" {
    try std.testing.fuzz({}, fuzzOne, .{});
}
fn fuzzOne(_: void, input: []const u8) anyerror!void {
    const gpa = std.testing.allocator;
    inline for (.{ false, true }) |d| {
        var r: Reader = .{ .gpa = gpa, .deflate = d, .max_message = 1 << 16 };
        defer r.deinit();
        var got: Got = .{ .gpa = gpa };
        defer got.deinit();
        r.feed(input, &got) catch {};
    }
}
