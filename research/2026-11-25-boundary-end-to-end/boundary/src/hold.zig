//! A synchronous precaution at the boundary: #353's destructive-call detector, with #383's rules,
//! asked about a request before it leaves, and a hold until a person answers.
//!
//! The detector runs outside the boundary, in its own process (`precaution/precaution.py`), as an
//! extension's classifier would (`synchronous-precaution-path`, `where-rules-run`). The boundary
//! keeps both limits itself (`classifier-wait-limit`, `hold-limit`): the wait for the detector's
//! opinion, and the wait for the person's answer. Nothing the other process does lengthens them.
//!
//! The protocol is one Unix connection per request, lines of JSON:
//!   boundary -> {"computer","host","method","target","headers":[[k,v]...],"body_b64"}
//!   precaution -> {"decision":"hold"|"allow","verdict":...,"says":[...]}
//!   and, after a hold, when the person answers: {"answer":"yes"|"no"}
const std = @import("std");
const posix = std.posix;

pub const Opinion = struct {
    decision: []const u8 = "allow",
    verdict: []const u8 = "not covered",
    says: []const []const u8 = &.{},
};

pub const Outcome = union(enum) {
    /// The detector does not hold it.
    allow: Opinion,
    /// Held, and a person said yes.
    yes: Opinion,
    /// Held, and a person said no.
    no: Opinion,
    /// Held, and no answer came before the hold's limit.
    unanswered: Opinion,
    /// No opinion before the classifier's limit, or the precaution could not be reached.
    no_opinion: []const u8,
};

pub const Ask = struct {
    sock: []const u8,
    classify_ms: u32,
    hold_ms: u32,
    /// Called once the request is held, before the wait for the person's answer: the boundary
    /// records the hold here. An error refuses the request without waiting.
    on_hold: *const fn (ctx: *anyopaque, op: Opinion) anyerror!void,
    ctx: *anyopaque,
};

fn setTimeout(fd: posix.fd_t, ms: u32) void {
    const tv: posix.timeval = .{ .sec = @intCast(ms / 1000), .usec = @intCast((ms % 1000) * 1000) };
    posix.setsockopt(fd, posix.SOL.SOCKET, posix.SO.RCVTIMEO, std.mem.asBytes(&tv)) catch {};
}

/// Reads one line, ending at `deadline_ms` (monotonic) whatever the other side does.
fn readLine(fd: posix.fd_t, buf: *std.ArrayList(u8), gpa: std.mem.Allocator, deadline_ms: i64) !?[]const u8 {
    while (true) {
        if (std.mem.indexOfScalar(u8, buf.items, '\n')) |i| return buf.items[0..i];
        const left = deadline_ms - std.time.milliTimestamp();
        if (left <= 0) return null;
        setTimeout(fd, @intCast(@max(left, 1)));
        var tmp: [4096]u8 = undefined;
        const n = posix.read(fd, &tmp) catch |e| switch (e) {
            error.WouldBlock => return null,
            else => return e,
        };
        if (n == 0) return error.Closed;
        try buf.appendSlice(gpa, tmp[0..n]);
        if (buf.items.len > 1 << 20) return error.TooLong;
    }
}

pub fn ask(gpa: std.mem.Allocator, a: Ask, computer: []const u8, host: []const u8, head: []const u8, body: []const u8) Outcome {
    return askInner(gpa, a, computer, host, head, body) catch |e| .{ .no_opinion = @errorName(e) };
}

fn askInner(gpa: std.mem.Allocator, a: Ask, computer: []const u8, host: []const u8, head: []const u8, body: []const u8) !Outcome {
    const t0 = std.time.milliTimestamp();
    const fd = try posix.socket(posix.AF.UNIX, posix.SOCK.STREAM | posix.SOCK.CLOEXEC, 0);
    defer posix.close(fd);
    var addr = try std.net.Address.initUnix(a.sock);
    try posix.connect(fd, &addr.any, addr.getOsSockLen());

    var out: std.Io.Writer.Allocating = .init(gpa);
    defer out.deinit();
    const w = &out.writer;
    var lines = std.mem.splitSequence(u8, head, "\r\n");
    const first = lines.next() orelse "";
    var fp = std.mem.splitScalar(u8, first, ' ');
    const method = fp.next() orelse "";
    const target = fp.next() orelse "";
    try w.print("{{\"computer\":{f},\"host\":{f},\"method\":{f},\"target\":{f},\"headers\":[", .{ std.json.fmt(computer, .{}), std.json.fmt(host, .{}), std.json.fmt(method, .{}), std.json.fmt(target, .{}) });
    var n: usize = 0;
    while (lines.next()) |l| {
        if (l.len == 0) break;
        const colon = std.mem.indexOfScalar(u8, l, ':') orelse continue;
        try w.print("{s}[{f},{f}]", .{ if (n > 0) "," else "", std.json.fmt(std.mem.trim(u8, l[0..colon], " "), .{}), std.json.fmt(std.mem.trim(u8, l[colon + 1 ..], " "), .{}) });
        n += 1;
    }
    try w.writeAll("],\"body_b64\":\"");
    try std.base64.standard.Encoder.encodeWriter(w, body);
    try w.writeAll("\"}\n");
    const msg = out.written();
    var off: usize = 0;
    while (off < msg.len) off += try posix.write(fd, msg[off..]);

    var buf: std.ArrayList(u8) = .empty;
    defer buf.deinit(gpa);
    const line = (try readLine(fd, &buf, gpa, t0 + a.classify_ms)) orelse return .{ .no_opinion = "classifier-limit" };
    const parsed = try std.json.parseFromSlice(Opinion, gpa, line, .{ .ignore_unknown_fields = true, .allocate = .alloc_always });
    // The opinion's strings live until the request ends; the caller's allocator is per request.
    const op = parsed.value;
    if (!std.mem.eql(u8, op.decision, "hold")) return .{ .allow = op };
    try a.on_hold(a.ctx, op);
    const consumed = line.len + 1;
    std.mem.copyForwards(u8, buf.items[0 .. buf.items.len - consumed], buf.items[consumed..]);
    buf.shrinkRetainingCapacity(buf.items.len - consumed);
    const t_hold = std.time.milliTimestamp();
    const ans = (readLine(fd, &buf, gpa, t_hold + a.hold_ms) catch null) orelse return .{ .unanswered = op };
    if (std.mem.indexOf(u8, ans, "\"yes\"") != null) return .{ .yes = op };
    return .{ .no = op };
}

test "a precaution that never answers is ended at the classifier's limit" {
    const gpa = std.testing.allocator;
    var tmp = std.testing.tmpDir(.{});
    defer tmp.cleanup();
    var pbuf: [std.fs.max_path_bytes]u8 = undefined;
    const dir = try tmp.dir.realpath(".", &pbuf);
    var sbuf: [256]u8 = undefined;
    const sock = try std.fmt.bufPrint(&sbuf, "{s}/p.sock", .{dir});
    const lfd = try posix.socket(posix.AF.UNIX, posix.SOCK.STREAM, 0);
    defer posix.close(lfd);
    var addr = try std.net.Address.initUnix(sock);
    try posix.bind(lfd, &addr.any, addr.getOsSockLen());
    try posix.listen(lfd, 4);
    const Dummy = struct {
        fn onHold(_: *anyopaque, _: Opinion) anyerror!void {}
    };
    var x: u8 = 0;
    const t0 = std.time.milliTimestamp();
    const r = ask(gpa, .{ .sock = sock, .classify_ms = 200, .hold_ms = 200, .on_hold = Dummy.onHold, .ctx = &x }, "c1", "api.github.com", "DELETE /repos/a/b HTTP/1.1\r\nHost: api.github.com\r\n\r\n", "");
    const took = std.time.milliTimestamp() - t0;
    try std.testing.expectEqualStrings("classifier-limit", r.no_opinion);
    try std.testing.expect(took >= 190 and took < 1000);
}
