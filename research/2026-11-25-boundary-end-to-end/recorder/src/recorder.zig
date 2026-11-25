//! The recorder, as far as the boundary needs one: it takes lines from sources over #370's
//! sources' protocol (a 4-byte little-endian length, then one JSON object) and writes each
//! computer's chain with #386's writer, unchanged. A line is acknowledged only once its batch is
//! flushed. The secrets' values are given to the writer's redactor, so a value that reached a line
//! by mistake would be written as `[secret:<name>]`; the boundary is built never to send one.
//!
//!   recorder <config.json> <record dir> <socket>
//!
//! RECORDER_FLUSH_DELAY_MS slows each flush, to stand for a slow disk in the failure tests.
const std = @import("std");
const writer = @import("writer.zig");
const redact = @import("redact.zig");
const posix = std.posix;
const gpa = std.heap.c_allocator;

var mu: std.Thread.Mutex = .{};
var writers: std.StringHashMapUnmanaged(*writer.Writer) = .empty;
var record_dir: []const u8 = "";
var redactor: redact.Redactor = .{};
var flush_delay_ms: u64 = 0;

fn writerFor(computer: []const u8) !*writer.Writer {
    if (writers.get(computer)) |w| return w;
    const path = try std.fs.path.join(gpa, &.{ record_dir, computer });
    const w = try gpa.create(writer.Writer);
    w.* = try writer.Writer.open(gpa, path, computer, .{ .redactor = redactor, .head_every = 1000 });
    try writers.put(gpa, try gpa.dupe(u8, computer), w);
    return w;
}

fn readExact(fd: posix.fd_t, b: []u8) !void {
    var off: usize = 0;
    while (off < b.len) {
        const r = try posix.read(fd, b[off..]);
        if (r == 0) return error.Closed;
        off += r;
    }
}

fn recvFrame(fd: posix.fd_t) ![]u8 {
    var len: [4]u8 = undefined;
    try readExact(fd, &len);
    const n = std.mem.readInt(u32, &len, .little);
    if (n > 1 << 20) return error.TooLarge;
    const b = try gpa.alloc(u8, n);
    errdefer gpa.free(b);
    try readExact(fd, b);
    return b;
}

fn sendFrame(fd: posix.fd_t, m: []const u8) !void {
    var len: [4]u8 = undefined;
    std.mem.writeInt(u32, &len, @intCast(m.len), .little);
    _ = try posix.send(fd, &len, posix.MSG.NOSIGNAL);
    var off: usize = 0;
    while (off < m.len) off += try posix.send(fd, m[off..], posix.MSG.NOSIGNAL);
}

fn readable(fd: posix.fd_t) bool {
    var p = [_]posix.pollfd{.{ .fd = fd, .events = posix.POLL.IN, .revents = 0 }};
    return (posix.poll(&p, 0) catch 0) > 0;
}

fn serve(fd: posix.fd_t) void {
    defer posix.close(fd);
    serveInner(fd) catch |e| std.log.warn("source closed: {s}", .{@errorName(e)});
}

fn serveInner(fd: posix.fd_t) !void {
    const hello_bytes = try recvFrame(fd);
    defer gpa.free(hello_bytes);
    const hello = try std.json.parseFromSlice(std.json.Value, gpa, hello_bytes, .{});
    defer hello.deinit();
    const h = hello.value.object.get("hello") orelse return error.NoHello;
    const source = try gpa.dupe(u8, h.object.get("source").?.string);
    defer gpa.free(source);
    var cb: [24]u8 = undefined;
    const count = if (h.object.get("count")) |cv| try gpa.dupe(u8, cv.string) else blk: {
        var r: [4]u8 = undefined;
        std.crypto.random.bytes(&r);
        break :blk try gpa.dupe(u8, std.fmt.bufPrint(&cb, "k:{x}", .{r}) catch unreachable);
    };
    defer gpa.free(count);
    {
        const m = try std.fmt.allocPrint(gpa, "{{\"count\":\"{s}\"}}", .{count});
        defer gpa.free(m);
        try sendFrame(fd, m);
    }
    while (true) {
        var batch: std.ArrayList([]u8) = .empty;
        defer {
            for (batch.items) |b| gpa.free(b);
            batch.deinit(gpa);
        }
        try batch.append(gpa, try recvFrame(fd));
        while (batch.items.len < 4096 and readable(fd)) try batch.append(gpa, try recvFrame(fd));
        var last: u64 = 0;
        mu.lock();
        defer mu.unlock();
        var touched: std.ArrayList(*writer.Writer) = .empty;
        defer touched.deinit(gpa);
        for (batch.items) |m| {
            const p = try std.json.parseFromSlice(std.json.Value, gpa, m, .{});
            defer p.deinit();
            const l = p.value.object.get("line") orelse continue; // "last": nothing to write
            const o = l.object;
            const n: u64 = @intCast(o.get("n").?.integer);
            const w = try writerFor(o.get("computer").?.string);
            const keys = try std.json.Stringify.valueAlloc(gpa, o.get("keys").?, .{});
            defer gpa.free(keys);
            var body: std.ArrayList(u8) = .empty;
            defer body.deinit(gpa);
            _ = try redactor.apply(gpa, &body, keys[1 .. keys.len - 1]);
            _ = try w.append(.{
                .time_ns = o.get("time_ns").?.integer,
                .source = source,
                .count = count,
                .n = n,
                .is = o.get("is").?.string,
                .kind = o.get("kind").?.string,
                .body = body.items,
            });
            last = @max(last, n);
            if (std.mem.indexOfScalar(*writer.Writer, touched.items, w) == null) try touched.append(gpa, w);
        }
        if (flush_delay_ms > 0) std.Thread.sleep(flush_delay_ms * std.time.ns_per_ms);
        for (touched.items) |w| try w.flush();
        if (last > 0) {
            var ab: [48]u8 = undefined;
            try sendFrame(fd, std.fmt.bufPrint(&ab, "{{\"ack\":{d}}}", .{last}) catch unreachable);
        }
    }
}

fn stop(_: c_int) callconv(.c) void {
    mu.lock();
    var it = writers.valueIterator();
    while (it.next()) |w| {
        w.*.flush() catch {};
        w.*.writeHead() catch {};
    }
    posix.exit(0);
}

pub fn main() !void {
    const args = try std.process.argsAlloc(gpa);
    if (args.len != 4) return error.Usage;
    record_dir = args[2];
    if (posix.getenv("RECORDER_FLUSH_DELAY_MS")) |d| flush_delay_ms = try std.fmt.parseInt(u64, d, 10);
    const bytes = try std.fs.cwd().readFileAlloc(gpa, args[1], 1 << 20);
    const parsed = try std.json.parseFromSlice(std.json.Value, gpa, bytes, .{});
    var secrets: std.ArrayList(redact.Secret) = .empty;
    if (parsed.value.object.get("boundary")) |b| for (b.object.get("computers").?.array.items) |cp| {
        if (cp.object.get("secrets")) |ss| for (ss.array.items) |s| try secrets.append(gpa, .{ .name = s.object.get("name").?.string, .value = s.object.get("value").?.string });
    };
    redactor = try redact.Redactor.init(secrets.items);
    var act: posix.Sigaction = .{ .handler = .{ .handler = stop }, .mask = posix.sigemptyset(), .flags = 0 };
    posix.sigaction(posix.SIG.TERM, &act, null);
    posix.sigaction(posix.SIG.INT, &act, null);

    std.fs.cwd().deleteFile(args[3]) catch {};
    const lfd = try posix.socket(posix.AF.UNIX, posix.SOCK.STREAM | posix.SOCK.CLOEXEC, 0);
    var addr = try std.net.Address.initUnix(args[3]);
    try posix.bind(lfd, &addr.any, addr.getOsSockLen());
    try posix.listen(lfd, 64);
    while (true) {
        const fd = try posix.accept(lfd, null, null, posix.SOCK.CLOEXEC);
        (try std.Thread.spawn(.{}, serve, .{fd})).detach();
    }
}
