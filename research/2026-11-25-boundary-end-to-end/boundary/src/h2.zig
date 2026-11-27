//! HTTP/2 on a host Silo reads, for programs that cannot fall back to HTTP/1.1: gRPC, which Google's
//! Go and Node cloud libraries use (#379). Silo ends HTTP/2 on both sides with nghttp2: each
//! request's headers go through the same checks and swap as an HTTP/1.1 head (`swap.zig`), then
//! the request is sent on Silo's own HTTP/2 connection to the host, and its body, the response,
//! its body and its trailers are carried stream by stream.
//!
//! The headers reach the checks as one block of lines `name\x00value\n`, so the swap that replaces
//! placeholders in an HTTP/1.1 head runs unchanged. Bodies are not read: a placeholder in a body is
//! not swapped, as on HTTP/1.1. Model hosts stay on HTTP/1.1, where #374 counts their calls.
const std = @import("std");
const c = @import("mint.zig").c;

pub const Verdict = union(enum) {
    /// The header block to send, swapped, allocated with the allocator given to `relay`.
    send: []u8,
    /// Silo's sentence: the stream is answered by Silo and nothing leaves.
    refuse: []const u8,
};

pub const Hooks = struct {
    ctx: *anyopaque,
    /// Called once per request, with its header block. Decides and records.
    request: *const fn (ctx: *anyopaque, block: []const u8) Verdict,
};

const Buf = std.ArrayList(u8);

const Stream = struct {
    srv_id: i32,
    up_id: i32 = -1,
    req_hdrs: Buf = .empty,
    req_trailers: Buf = .empty,
    req_headers_done: bool = false,
    req_body: Buf = .empty,
    req_off: usize = 0,
    req_end: bool = false,
    resp_hdrs: Buf = .empty,
    resp_trailers: Buf = .empty,
    resp_headers_done: bool = false,
    resp_submitted: bool = false,
    resp_body: Buf = .empty,
    resp_off: usize = 0,
    resp_end: bool = false,
    refusal: ?[]const u8 = null,
    refusal_sent: bool = false,
    grpc: bool = false,
};

const Conn = struct {
    a: std.mem.Allocator,
    gpa: std.mem.Allocator,
    srv: ?*c.nghttp2_session = null,
    up: ?*c.nghttp2_session = null,
    hooks: Hooks,
    failed: bool = false,
    sent: u64 = 0,
    received: u64 = 0,
    streams: u32 = 0,
};

pub const Totals = struct { streams: u32, sent: u64, received: u64 };

fn connOf(p: ?*anyopaque) *Conn {
    return @ptrCast(@alignCast(p.?));
}

fn streamOf(s: ?*c.nghttp2_session, id: i32) ?*Stream {
    const p = c.nghttp2_session_get_stream_user_data(s, id) orelse return null;
    return @ptrCast(@alignCast(p));
}

/// The name-value array nghttp2 sends, pointing into a block of `name\x00value\n` lines.
fn nvOf(a: std.mem.Allocator, block: []const u8, skip_grpc_status: bool) ![]c.nghttp2_nv {
    var list: std.ArrayList(c.nghttp2_nv) = .empty;
    var it = std.mem.splitScalar(u8, block, '\n');
    while (it.next()) |line| {
        if (line.len == 0) continue;
        const z = std.mem.indexOfScalar(u8, line, 0) orelse continue;
        const name = line[0..z];
        const value = line[z + 1 ..];
        if (skip_grpc_status and std.mem.eql(u8, name, "grpc-status")) continue;
        try list.append(a, .{
            .name = @constCast(name.ptr),
            .namelen = name.len,
            .value = @constCast(value.ptr),
            .valuelen = value.len,
            .flags = c.NGHTTP2_NV_FLAG_NONE,
        });
    }
    return list.toOwnedSlice(a);
}

fn addLine(a: std.mem.Allocator, b: *Buf, name: []const u8, value: []const u8) !void {
    try b.appendSlice(a, name);
    try b.append(a, 0);
    try b.appendSlice(a, value);
    try b.append(a, '\n');
}

// ---- reading bodies out to the other side ----

fn readReq(_: ?*c.nghttp2_session, stream_id: i32, buf: [*c]u8, len: usize, flags: [*c]u32, src: [*c]c.nghttp2_data_source, user: ?*anyopaque) callconv(.c) isize {
    const conn = connOf(user);
    const st: *Stream = @ptrCast(@alignCast(src.*.ptr.?));
    const avail = st.req_body.items.len - st.req_off;
    if (avail > 0) {
        const n = @min(avail, len);
        @memcpy(buf[0..n], st.req_body.items[st.req_off .. st.req_off + n]);
        st.req_off += n;
        if (st.req_off == st.req_body.items.len) {
            st.req_body.clearRetainingCapacity();
            st.req_off = 0;
        }
        conn.sent += n;
        return @intCast(n);
    }
    if (!st.req_end) return c.NGHTTP2_ERR_DEFERRED;
    flags.* |= c.NGHTTP2_DATA_FLAG_EOF;
    if (st.req_trailers.items.len > 0) {
        flags.* |= c.NGHTTP2_DATA_FLAG_NO_END_STREAM;
        const nv = nvOf(conn.a, st.req_trailers.items, false) catch return c.NGHTTP2_ERR_CALLBACK_FAILURE;
        _ = c.nghttp2_submit_trailer(conn.up, stream_id, nv.ptr, nv.len);
    }
    return 0;
}

fn readResp(_: ?*c.nghttp2_session, stream_id: i32, buf: [*c]u8, len: usize, flags: [*c]u32, src: [*c]c.nghttp2_data_source, user: ?*anyopaque) callconv(.c) isize {
    const conn = connOf(user);
    const st: *Stream = @ptrCast(@alignCast(src.*.ptr.?));
    const avail = st.resp_body.items.len - st.resp_off;
    if (avail > 0) {
        const n = @min(avail, len);
        @memcpy(buf[0..n], st.resp_body.items[st.resp_off .. st.resp_off + n]);
        st.resp_off += n;
        if (st.resp_off == st.resp_body.items.len) {
            st.resp_body.clearRetainingCapacity();
            st.resp_off = 0;
        }
        return @intCast(n);
    }
    if (!st.resp_end) return c.NGHTTP2_ERR_DEFERRED;
    flags.* |= c.NGHTTP2_DATA_FLAG_EOF;
    if (st.resp_trailers.items.len > 0) {
        flags.* |= c.NGHTTP2_DATA_FLAG_NO_END_STREAM;
        const nv = nvOf(conn.a, st.resp_trailers.items, false) catch return c.NGHTTP2_ERR_CALLBACK_FAILURE;
        _ = c.nghttp2_submit_trailer(conn.srv, stream_id, nv.ptr, nv.len);
    }
    return 0;
}

/// Silo's own answer on a stream it refuses: gRPC's PERMISSION_DENIED for a gRPC call, which a
/// gRPC client shows as the message, and 403 with the sentence otherwise.
fn refuse(conn: *Conn, st: *Stream) !void {
    const msg = st.refusal.?;
    var b: Buf = .empty;
    if (st.grpc) {
        try addLine(conn.a, &b, ":status", "200");
        try addLine(conn.a, &b, "content-type", "application/grpc");
        try addLine(conn.a, &b, "grpc-status", "7");
        try addLine(conn.a, &b, "grpc-message", msg);
        const nv = try nvOf(conn.a, b.items, false);
        _ = c.nghttp2_submit_response(conn.srv, st.srv_id, nv.ptr, nv.len, null);
    } else {
        try addLine(conn.a, &b, ":status", "403");
        try addLine(conn.a, &b, "content-type", "text/plain");
        const nv = try nvOf(conn.a, b.items, false);
        try st.resp_body.appendSlice(conn.a, msg);
        st.resp_end = true;
        var dp: c.nghttp2_data_provider = .{ .source = .{ .ptr = st }, .read_callback = readResp };
        _ = c.nghttp2_submit_response(conn.srv, st.srv_id, nv.ptr, nv.len, &dp);
    }
    st.refusal_sent = true;
}

// ---- the program's side: Silo is the server ----

fn srvBeginHeaders(s: ?*c.nghttp2_session, frame: [*c]const c.nghttp2_frame, user: ?*anyopaque) callconv(.c) c_int {
    const conn = connOf(user);
    if (frame.*.hd.type != c.NGHTTP2_HEADERS or frame.*.headers.cat != c.NGHTTP2_HCAT_REQUEST) return 0;
    const st = conn.a.create(Stream) catch return c.NGHTTP2_ERR_CALLBACK_FAILURE;
    st.* = .{ .srv_id = frame.*.hd.stream_id };
    conn.streams += 1;
    _ = c.nghttp2_session_set_stream_user_data(s, frame.*.hd.stream_id, st);
    return 0;
}

fn srvHeader(s: ?*c.nghttp2_session, frame: [*c]const c.nghttp2_frame, name: [*c]const u8, nl: usize, value: [*c]const u8, vl: usize, _: u8, user: ?*anyopaque) callconv(.c) c_int {
    const conn = connOf(user);
    const st = streamOf(s, frame.*.hd.stream_id) orelse return 0;
    const n = name[0..nl];
    const v = value[0..vl];
    if (std.mem.eql(u8, n, "content-type") and std.mem.startsWith(u8, v, "application/grpc")) st.grpc = true;
    addLine(conn.a, if (st.req_headers_done) &st.req_trailers else &st.req_hdrs, n, v) catch return c.NGHTTP2_ERR_CALLBACK_FAILURE;
    return 0;
}

fn srvFrame(s: ?*c.nghttp2_session, frame: [*c]const c.nghttp2_frame, user: ?*anyopaque) callconv(.c) c_int {
    const conn = connOf(user);
    const st = streamOf(s, frame.*.hd.stream_id) orelse return 0;
    const end = frame.*.hd.flags & c.NGHTTP2_FLAG_END_STREAM != 0;
    switch (frame.*.hd.type) {
        c.NGHTTP2_HEADERS => {
            if (st.req_headers_done) {
                st.req_end = true; // trailers
            } else {
                st.req_headers_done = true;
                if (end) st.req_end = true;
                switch (conn.hooks.request(conn.hooks.ctx, st.req_hdrs.items)) {
                    .refuse => |m| {
                        st.refusal = m;
                        refuse(conn, st) catch return c.NGHTTP2_ERR_CALLBACK_FAILURE;
                        return 0;
                    },
                    .send => |block| {
                        // nghttp2 copies the headers when the request is submitted.
                        defer conn.gpa.free(block);
                        const nv = nvOf(conn.a, block, false) catch return c.NGHTTP2_ERR_CALLBACK_FAILURE;
                        var dp: c.nghttp2_data_provider = .{ .source = .{ .ptr = st }, .read_callback = readReq };
                        const id = c.nghttp2_submit_request(conn.up, null, nv.ptr, nv.len, if (end) null else &dp, st);
                        if (id < 0) return c.NGHTTP2_ERR_CALLBACK_FAILURE;
                        st.up_id = id;
                        return 0;
                    },
                }
            }
        },
        c.NGHTTP2_DATA => if (end) {
            st.req_end = true;
        },
        else => return 0,
    }
    if (st.up_id > 0) _ = c.nghttp2_session_resume_data(conn.up, st.up_id);
    return 0;
}

fn srvData(_: ?*c.nghttp2_session, _: u8, stream_id: i32, data: [*c]const u8, len: usize, user: ?*anyopaque) callconv(.c) c_int {
    const conn = connOf(user);
    const st = streamOf(conn.srv, stream_id) orelse return 0;
    if (st.refusal != null) return 0;
    st.req_body.appendSlice(conn.a, data[0..len]) catch return c.NGHTTP2_ERR_CALLBACK_FAILURE;
    if (st.up_id > 0) _ = c.nghttp2_session_resume_data(conn.up, st.up_id);
    return 0;
}

fn srvClose(_: ?*c.nghttp2_session, stream_id: i32, code: u32, user: ?*anyopaque) callconv(.c) c_int {
    const conn = connOf(user);
    const st = streamOf(conn.srv, stream_id) orelse return 0;
    if (st.up_id > 0 and code != 0) _ = c.nghttp2_submit_rst_stream(conn.up, c.NGHTTP2_FLAG_NONE, st.up_id, code);
    return 0;
}

// ---- the host's side: Silo is the client ----

fn upHeader(s: ?*c.nghttp2_session, frame: [*c]const c.nghttp2_frame, name: [*c]const u8, nl: usize, value: [*c]const u8, vl: usize, _: u8, user: ?*anyopaque) callconv(.c) c_int {
    const conn = connOf(user);
    const st = streamOf(s, frame.*.hd.stream_id) orelse return 0;
    addLine(conn.a, if (st.resp_headers_done) &st.resp_trailers else &st.resp_hdrs, name[0..nl], value[0..vl]) catch return c.NGHTTP2_ERR_CALLBACK_FAILURE;
    return 0;
}

fn upFrame(s: ?*c.nghttp2_session, frame: [*c]const c.nghttp2_frame, user: ?*anyopaque) callconv(.c) c_int {
    const conn = connOf(user);
    const st = streamOf(s, frame.*.hd.stream_id) orelse return 0;
    const end = frame.*.hd.flags & c.NGHTTP2_FLAG_END_STREAM != 0;
    switch (frame.*.hd.type) {
        c.NGHTTP2_HEADERS => {
            if (!st.resp_headers_done) {
                // A 1xx answer is not the response: wait for the next HEADERS.
                if (std.mem.startsWith(u8, st.resp_hdrs.items, ":status\x001")) {
                    st.resp_hdrs.clearRetainingCapacity();
                    return 0;
                }
                st.resp_headers_done = true;
                if (end) st.resp_end = true;
                const nv = nvOf(conn.a, st.resp_hdrs.items, false) catch return c.NGHTTP2_ERR_CALLBACK_FAILURE;
                var dp: c.nghttp2_data_provider = .{ .source = .{ .ptr = st }, .read_callback = readResp };
                if (c.nghttp2_submit_response(conn.srv, st.srv_id, nv.ptr, nv.len, if (end) null else &dp) != 0) return c.NGHTTP2_ERR_CALLBACK_FAILURE;
                st.resp_submitted = true;
                return 0;
            }
            st.resp_end = true; // trailers
        },
        c.NGHTTP2_DATA => if (end) {
            st.resp_end = true;
        },
        else => return 0,
    }
    if (st.resp_submitted) _ = c.nghttp2_session_resume_data(conn.srv, st.srv_id);
    return 0;
}

fn upData(_: ?*c.nghttp2_session, _: u8, stream_id: i32, data: [*c]const u8, len: usize, user: ?*anyopaque) callconv(.c) c_int {
    const conn = connOf(user);
    const st = streamOf(conn.up, stream_id) orelse return 0;
    conn.received += len;
    st.resp_body.appendSlice(conn.a, data[0..len]) catch return c.NGHTTP2_ERR_CALLBACK_FAILURE;
    if (st.resp_submitted) _ = c.nghttp2_session_resume_data(conn.srv, st.srv_id);
    return 0;
}

fn upClose(_: ?*c.nghttp2_session, stream_id: i32, code: u32, user: ?*anyopaque) callconv(.c) c_int {
    const conn = connOf(user);
    const st = streamOf(conn.up, stream_id) orelse return 0;
    if (code != 0) _ = c.nghttp2_submit_rst_stream(conn.srv, c.NGHTTP2_FLAG_NONE, st.srv_id, code);
    return 0;
}

// ---- the loop ----

fn flush(s: ?*c.nghttp2_session, ssl: ?*c.SSL) bool {
    while (true) {
        var p: [*c]const u8 = undefined;
        const n = c.nghttp2_session_mem_send(s, &p);
        if (n < 0) return false;
        if (n == 0) return true;
        var off: usize = 0;
        while (off < n) {
            const w = c.SSL_write(ssl, p + off, @intCast(@as(usize, @intCast(n)) - off));
            if (w <= 0) return false;
            off += @intCast(w);
        }
    }
}

fn feed(s: ?*c.nghttp2_session, ssl: ?*c.SSL, buf: []u8) bool {
    const r = c.SSL_read(ssl, buf.ptr, @intCast(buf.len));
    if (r <= 0) return false;
    return c.nghttp2_session_mem_recv(s, buf.ptr, @intCast(r)) >= 0;
}

fn alive(s: ?*c.nghttp2_session) bool {
    return c.nghttp2_session_want_read(s) != 0 or c.nghttp2_session_want_write(s) != 0;
}

/// Carries one HTTP/2 connection of a program to its host, both already past TLS with "h2"
/// chosen. Returns when either side ends. Everything is allocated in an arena freed at the end.
pub fn relay(gpa: std.mem.Allocator, client: ?*c.SSL, client_fd: c_int, up: ?*c.SSL, up_fd: c_int, hooks: Hooks) !Totals {
    var arena = std.heap.ArenaAllocator.init(gpa);
    defer arena.deinit();
    var conn: Conn = .{ .a = arena.allocator(), .gpa = gpa, .hooks = hooks };

    var cbs: ?*c.nghttp2_session_callbacks = null;
    if (c.nghttp2_session_callbacks_new(&cbs) != 0) return error.OutOfMemory;
    defer c.nghttp2_session_callbacks_del(cbs);
    c.nghttp2_session_callbacks_set_on_begin_headers_callback(cbs, srvBeginHeaders);
    c.nghttp2_session_callbacks_set_on_header_callback(cbs, srvHeader);
    c.nghttp2_session_callbacks_set_on_frame_recv_callback(cbs, srvFrame);
    c.nghttp2_session_callbacks_set_on_data_chunk_recv_callback(cbs, srvData);
    c.nghttp2_session_callbacks_set_on_stream_close_callback(cbs, srvClose);
    if (c.nghttp2_session_server_new(&conn.srv, cbs, &conn) != 0) return error.OutOfMemory;
    defer c.nghttp2_session_del(conn.srv);

    var ucbs: ?*c.nghttp2_session_callbacks = null;
    if (c.nghttp2_session_callbacks_new(&ucbs) != 0) return error.OutOfMemory;
    defer c.nghttp2_session_callbacks_del(ucbs);
    c.nghttp2_session_callbacks_set_on_header_callback(ucbs, upHeader);
    c.nghttp2_session_callbacks_set_on_frame_recv_callback(ucbs, upFrame);
    c.nghttp2_session_callbacks_set_on_data_chunk_recv_callback(ucbs, upData);
    c.nghttp2_session_callbacks_set_on_stream_close_callback(ucbs, upClose);
    if (c.nghttp2_session_client_new(&conn.up, ucbs, &conn) != 0) return error.OutOfMemory;
    defer c.nghttp2_session_del(conn.up);

    var iv = [_]c.nghttp2_settings_entry{.{ .settings_id = c.NGHTTP2_SETTINGS_MAX_CONCURRENT_STREAMS, .value = 100 }};
    _ = c.nghttp2_submit_settings(conn.srv, c.NGHTTP2_FLAG_NONE, &iv, iv.len);
    _ = c.nghttp2_submit_settings(conn.up, c.NGHTTP2_FLAG_NONE, null, 0);

    var buf: [16384]u8 = undefined;
    while (true) {
        if (!flush(conn.srv, client) or !flush(conn.up, up)) break;
        if (!alive(conn.srv) or !alive(conn.up)) break;
        // TLS may hold bytes already read off the socket: use them before waiting.
        if (c.SSL_pending(client) > 0) {
            if (!feed(conn.srv, client, &buf)) break;
            continue;
        }
        if (c.SSL_pending(up) > 0) {
            if (!feed(conn.up, up, &buf)) break;
            continue;
        }
        var fds = [_]std.posix.pollfd{
            .{ .fd = client_fd, .events = std.posix.POLL.IN, .revents = 0 },
            .{ .fd = up_fd, .events = std.posix.POLL.IN, .revents = 0 },
        };
        _ = std.posix.poll(&fds, -1) catch break;
        if (fds[0].revents != 0 and !feed(conn.srv, client, &buf)) break;
        if (fds[1].revents != 0 and !feed(conn.up, up, &buf)) break;
    }
    _ = flush(conn.srv, client);
    return .{ .streams = conn.streams, .sent = conn.sent, .received = conn.received };
}
