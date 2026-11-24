//! A disk served by Silo over NBD, the route Apple's framework offers to a disk of Silo's own
//! (VZNetworkBlockDeviceStorageDeviceAttachment, macOS 14). Every write of the computer passes
//! through Silo, which marks it in a log; a checkpoint is then the copy-before-write of
//! strategy.zig's Cbw, and its moment falls between two requests, so the computer never waits
//! for it. Fixed newstyle handshake, simple replies, one connection: enough for nbdinfo, nbdcopy
//! and the bench's own client. Not a server to ship.
const std = @import("std");
const posix = std.posix;
const sys = @import("sys.zig");
const S = @import("strategy.zig");
const BS = @import("writer.zig").BS;

const NBDMAGIC: u64 = 0x4e42444d41474943;
const IHAVEOPT: u64 = 0x49484156454F5054;
const REPLY_MAGIC: u64 = 0x3e889045565a9;
const REQ_MAGIC: u32 = 0x25609513;
const SIMPLE_REPLY: u32 = 0x67446698;
const FLAG_FIXED_NEWSTYLE: u16 = 1;
const FLAG_NO_ZEROES: u16 = 2;
const TX_HAS_FLAGS: u16 = 1;
const TX_SEND_FLUSH: u16 = 4;
const TX_SEND_FUA: u16 = 8;
const OPT_EXPORT_NAME: u32 = 1;
const OPT_ABORT: u32 = 2;
const OPT_INFO: u32 = 6;
const OPT_GO: u32 = 7;
const REP_ACK: u32 = 1;
const REP_INFO: u32 = 3;
const REP_ERR_UNSUP: u32 = 0x80000001;
const CMD_READ: u16 = 0;
const CMD_WRITE: u16 = 1;
const CMD_DISC: u16 = 2;
const CMD_FLUSH: u16 = 3;
const CMD_FLAG_FUA: u16 = 1;
const MAX_REQ: usize = 32 << 20;

fn readAll(fd: posix.fd_t, buf: []u8) !void {
    var n: usize = 0;
    while (n < buf.len) {
        const r = try posix.read(fd, buf[n..]);
        if (r == 0) return error.Closed;
        n += r;
    }
}
fn writeAll(fd: posix.fd_t, buf: []const u8) !void {
    var n: usize = 0;
    while (n < buf.len) n += try posix.write(fd, buf[n..]);
}
fn be(comptime T: type, v: T) [@sizeOf(T)]u8 {
    var b: [@sizeOf(T)]u8 = undefined;
    std.mem.writeInt(T, &b, v, .big);
    return b;
}
fn rd(comptime T: type, b: []const u8) T {
    return std.mem.readInt(T, b[0..@sizeOf(T)], .big);
}

pub const Server = struct {
    img: posix.fd_t,
    size: u64,
    cbw: ?*S.Cbw,
    buf: []align(4096) u8,
    /// 1: a checkpoint asks for its moment; 2: the moment was taken.
    want_swap: std.atomic.Value(u32) = .init(0),
    requests: u64 = 0,

    pub fn init(gpa: std.mem.Allocator, path: []const u8, size: u64, cbw: ?*S.Cbw) !Server {
        return .{ .img = try sys.openImage(path, false), .size = size, .cbw = cbw, .buf = try gpa.alignedAlloc(u8, .fromByteUnits(4096), MAX_REQ) };
    }

    fn optReply(fd: posix.fd_t, opt: u32, typ: u32, data: []const u8) !void {
        var h: [20]u8 = undefined;
        @memcpy(h[0..8], &be(u64, REPLY_MAGIC));
        @memcpy(h[8..12], &be(u32, opt));
        @memcpy(h[12..16], &be(u32, typ));
        @memcpy(h[16..20], &be(u32, @intCast(data.len)));
        try writeAll(fd, &h);
        try writeAll(fd, data);
    }

    fn handshake(s: *Server, fd: posix.fd_t) !void {
        var h: [18]u8 = undefined;
        @memcpy(h[0..8], &be(u64, NBDMAGIC));
        @memcpy(h[8..16], &be(u64, IHAVEOPT));
        @memcpy(h[16..18], &be(u16, FLAG_FIXED_NEWSTYLE | FLAG_NO_ZEROES));
        try writeAll(fd, &h);
        var cf: [4]u8 = undefined;
        try readAll(fd, &cf);
        const no_zeroes = rd(u32, &cf) & FLAG_NO_ZEROES != 0;
        const txflags = TX_HAS_FLAGS | TX_SEND_FLUSH | TX_SEND_FUA;
        while (true) {
            var oh: [16]u8 = undefined;
            try readAll(fd, &oh);
            if (rd(u64, oh[0..8]) != IHAVEOPT) return error.BadOption;
            const opt = rd(u32, oh[8..12]);
            const len = rd(u32, oh[12..16]);
            if (len > 4096) return error.OptionTooLong;
            var data: [4096]u8 = undefined;
            try readAll(fd, data[0..len]);
            switch (opt) {
                OPT_EXPORT_NAME => {
                    var r: [10 + 124]u8 = @splat(0);
                    @memcpy(r[0..8], &be(u64, s.size));
                    @memcpy(r[8..10], &be(u16, txflags));
                    try writeAll(fd, if (no_zeroes) r[0..10] else r[0..]);
                    return;
                },
                OPT_INFO, OPT_GO => {
                    var info: [12]u8 = undefined;
                    @memcpy(info[0..2], &be(u16, 0)); // NBD_INFO_EXPORT
                    @memcpy(info[2..10], &be(u64, s.size));
                    @memcpy(info[10..12], &be(u16, txflags));
                    try optReply(fd, opt, REP_INFO, &info);
                    try optReply(fd, opt, REP_ACK, &.{});
                    if (opt == OPT_GO) return;
                },
                OPT_ABORT => {
                    try optReply(fd, opt, REP_ACK, &.{});
                    return error.Aborted;
                },
                else => try optReply(fd, opt, REP_ERR_UNSUP, &.{}),
            }
        }
    }

    fn reply(fd: posix.fd_t, handle: []const u8, err: u32) !void {
        var r: [16]u8 = undefined;
        @memcpy(r[0..4], &be(u32, SIMPLE_REPLY));
        @memcpy(r[4..8], &be(u32, err));
        @memcpy(r[8..16], handle[0..8]);
        try writeAll(fd, &r);
    }

    /// Serves one connection until the client leaves.
    pub fn serve(s: *Server, fd: posix.fd_t) !void {
        try s.handshake(fd);
        var fds = [_]posix.pollfd{.{ .fd = fd, .events = posix.POLL.IN, .revents = 0 }};
        while (true) {
            // Between two requests no write is in flight: the moment a checkpoint asked for.
            if (s.want_swap.load(.acquire) == 1) {
                s.cbw.?.swap();
                s.want_swap.store(2, .release);
            }
            if (try posix.poll(&fds, 1) == 0) continue;
            var h: [28]u8 = undefined;
            readAll(fd, &h) catch return;
            if (rd(u32, h[0..4]) != REQ_MAGIC) return error.BadRequest;
            const flags = rd(u16, h[4..6]);
            const typ = rd(u16, h[6..8]);
            const handle = h[8..16];
            const off = rd(u64, h[16..24]);
            const len = rd(u32, h[24..28]);
            s.requests += 1;
            switch (typ) {
                CMD_READ => {
                    if (len > MAX_REQ or off + len > s.size) return error.BadRequest;
                    _ = try posix.pread(s.img, s.buf[0..len], off);
                    try reply(fd, handle, 0);
                    try writeAll(fd, s.buf[0..len]);
                },
                CMD_WRITE => {
                    if (len > MAX_REQ or off + len > s.size) return error.BadRequest;
                    try readAll(fd, s.buf[0..len]);
                    if (s.cbw) |c| {
                        var b = off / BS;
                        while (b * BS < off + len) : (b += 1) S.Cbw.before(c, @intCast(b));
                    }
                    _ = try posix.pwrite(s.img, s.buf[0..len], off);
                    if (flags & CMD_FLAG_FUA != 0) try sys.fullSync(s.img);
                    try reply(fd, handle, 0);
                },
                CMD_FLUSH => {
                    try sys.fullSync(s.img);
                    try reply(fd, handle, 0);
                },
                CMD_DISC => return,
                else => try reply(fd, handle, 22), // EINVAL
            }
        }
    }

    /// A checkpoint of the served disk, from another thread: prepare, ask the serving thread for
    /// the moment, copy. Returns how long the moment took to come.
    pub fn checkpoint(s: *Server, prev: [:0]const u8, tmp: [:0]const u8, dst: [:0]const u8, dir: []const u8) !S.Result {
        const c = s.cbw.?;
        const t0 = sys.now();
        try c.prepare(prev, tmp);
        const clone_end = sys.now();
        s.want_swap.store(1, .release);
        while (s.want_swap.load(.acquire) != 2) std.atomic.spinLoopHint();
        const wait = sys.now() - clone_end;
        s.want_swap.store(0, .release);
        c.drain(s.size / BS);
        const op = sys.now() - t0;
        const publish_at = sys.now();
        try S.publish(tmp, dst, dir);
        return .{ .op_ns = op, .clone_end = clone_end, .publish_at = publish_at, .pause_ns = wait, .bytes_copied = c.bytes, .cow_copies = c.cow_copies };
    }
};

/// The computer's side, for the bench: the handshake, then one 4 KiB write at a time.
pub const Client = struct {
    fd: posix.fd_t,
    handle: u64 = 0,

    pub fn connect(fd: posix.fd_t) !Client {
        var h: [18]u8 = undefined;
        try readAll(fd, &h);
        if (rd(u64, h[0..8]) != NBDMAGIC or rd(u64, h[8..16]) != IHAVEOPT) return error.NotNbd;
        try writeAll(fd, &be(u32, FLAG_FIXED_NEWSTYLE | FLAG_NO_ZEROES));
        var o: [16]u8 = undefined;
        @memcpy(o[0..8], &be(u64, IHAVEOPT));
        @memcpy(o[8..12], &be(u32, OPT_EXPORT_NAME));
        @memcpy(o[12..16], &be(u32, 0));
        try writeAll(fd, &o);
        var r: [10]u8 = undefined;
        try readAll(fd, &r);
        return .{ .fd = fd };
    }

    pub fn write(c: *Client, data: []const u8, off: u64) !void {
        c.handle += 1;
        var h: [28]u8 = undefined;
        @memcpy(h[0..4], &be(u32, REQ_MAGIC));
        @memcpy(h[4..6], &be(u16, 0));
        @memcpy(h[6..8], &be(u16, CMD_WRITE));
        @memcpy(h[8..16], &be(u64, c.handle));
        @memcpy(h[16..24], &be(u64, off));
        @memcpy(h[24..28], &be(u32, @intCast(data.len)));
        try writeAll(c.fd, &h);
        try writeAll(c.fd, data);
        var r: [16]u8 = undefined;
        try readAll(c.fd, &r);
        if (rd(u32, r[0..4]) != SIMPLE_REPLY or rd(u32, r[4..8]) != 0 or rd(u64, r[8..16]) != c.handle) return error.WriteFailed;
    }
};

/// `ckpt nbd-serve image=PATH sock=PATH`: serves one image on a Unix socket, for nbdinfo and
/// nbdcopy (nbd+unix:///?socket=PATH). One client after another; no log.
pub const ServeOpts = struct { image: []const u8 = "", sock: []const u8 = "/tmp/ckpt-nbd.sock" };

pub fn serveMain(gpa: std.mem.Allocator, o: ServeOpts) !void {
    const st = try std.fs.cwd().statFile(o.image);
    var s = try Server.init(gpa, o.image, st.size, null);
    std.fs.cwd().deleteFile(o.sock) catch {};
    const l = try posix.socket(posix.AF.UNIX, posix.SOCK.STREAM, 0);
    var addr = try std.net.Address.initUnix(o.sock);
    try posix.bind(l, &addr.any, addr.getOsSockLen());
    try posix.listen(l, 4);
    while (true) {
        const c = try posix.accept(l, null, null, 0);
        s.serve(c) catch |e| std.debug.print("nbd: {s}\n", .{@errorName(e)});
        posix.close(c);
        std.debug.print("nbd: client left after {d} requests\n", .{s.requests});
    }
}
