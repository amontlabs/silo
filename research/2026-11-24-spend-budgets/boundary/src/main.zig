//! A spike of Silo's boundary for model traffic. It ends the TLS connection a program opens to a
//! provider's host (known by SNI), opens its own to the host and checks the host's certificate
//! against its own store, and forwards every byte as it came. It reads a copy: the request's tool
//! results, and the reply's events. A reply is held back from the moment a tool call starts until
//! the call is whole and the precaution has answered; on a deny, the probe is armed (and answers)
//! before the reply goes on, unchanged. The probe is disarmed when a request carries the result.
//!
//! This copy adds budgets (research of 24 November 2026, spend budgets at the boundary): it counts
//! each model call from the provider's own counts, prices it from Silo's own table, admits each
//! call against the budgets of its computer and its person before it leaves, and refuses a call
//! that may not leave with HTTP 400 in the provider's own error shape.
const std = @import("std");
const stream = @import("stream.zig");
const ws = @import("ws.zig");
const usage = @import("usage.zig");
const prices = @import("prices.zig");
const budget = @import("budget.zig");
const spend = @import("spend.zig");
const c = @cImport({
    @cInclude("openssl/ssl.h");
    @cInclude("openssl/err.h");
    @cInclude("zlib.h");
    @cInclude("zstd.h");
});
const posix = std.posix;
const gpa = std.heap.c_allocator;

const Config = struct {
    port: u16,
    upstream_ip: []const u8,
    upstream_port: u16,
    deny_substr: ?[]const u8,
    probe_ctl: ?[]const u8,
    probe_ack: ?[]const u8,
    record: []const u8,
    hold: bool,
};
var cfg: Config = undefined;
var server_ctx: ?*c.SSL_CTX = null;
var client_ctx: ?*c.SSL_CTX = null;
var conn_counter = std.atomic.Value(u32).init(0);

// ---- the record ----
var record_mu: std.Thread.Mutex = .{};
var record_fd: posix.fd_t = -1;

fn now_ms() f64 {
    return @as(f64, @floatFromInt(std.time.nanoTimestamp())) / 1e6;
}

fn record(comptime fmt: []const u8, args: anytype) void {
    const line = std.fmt.allocPrint(gpa, "{{\"t\":{d:.3}," ++ fmt ++ "}}\n", .{now_ms()} ++ args) catch return;
    defer gpa.free(line);
    record_mu.lock();
    defer record_mu.unlock();
    var off: usize = 0;
    while (off < line.len) off += posix.write(record_fd, line[off..]) catch return;
}

/// A JSON string with the escapes a record line needs.
const J = struct {
    s: []const u8,
    pub fn format(self: J, w: *std.Io.Writer) std.Io.Writer.Error!void {
        try w.writeByte('"');
        for (self.s) |ch| switch (ch) {
            '"' => try w.writeAll("\\\""),
            '\\' => try w.writeAll("\\\\"),
            0...0x1f => try w.print("\\u{x:0>4}", .{ch}),
            else => try w.writeByte(ch),
        };
        try w.writeByte('"');
    }
};
fn j(s: []const u8) J {
    return .{ .s = s };
}

fn sha(bytes: []const u8) [64]u8 {
    var h: [32]u8 = undefined;
    std.crypto.hash.sha2.Sha256.hash(bytes, &h, .{});
    return std.fmt.bytesToHex(h, .lower);
}

// ---- budgets ----
const State = struct {
    cfg: spend.Config,
    book: budget.Book,
    table: prices.Table,
    /// Per conversation: the size of its last request and the input the provider counted for it.
    memory: std.AutoHashMapUnmanaged(u64, struct { bytes: usize, tokens: i64 }) = .empty,
    memory_mu: std.Thread.Mutex = .{},
    /// For each computer of the config: the ledgers that cover its calls (its own, its person's).
    covers: [][2]usize = &.{},
    covers_len: []usize = &.{},
    budgeted: []bool = &.{},
    next_call: std.atomic.Value(u64) = .init(1),
    /// The calls under way, so an admission can bring their counts up to the moment.
    live: std.ArrayList(*Call) = .empty,
    /// The output of the last settled calls, per model, for `reserve_typical`.
    outputs: std.StringHashMapUnmanaged(std.ArrayList(i64)) = .empty,
    live_mu: std.Thread.Mutex = .{},
};

fn typicalOutput(st: *State, model: []const u8) i64 {
    st.live_mu.lock();
    defer st.live_mu.unlock();
    const l = st.outputs.get(model) orelse return st.cfg.typical_output_tokens;
    if (l.items.len < 5) return st.cfg.typical_output_tokens;
    const tmp = gpa.dupe(i64, l.items) catch return st.cfg.typical_output_tokens;
    defer gpa.free(tmp);
    std.mem.sort(i64, tmp, {}, std.sort.asc(i64));
    return tmp[(tmp.len * 9) / 10];
}

fn noteOutput(st: *State, model: []const u8, out: i64) void {
    st.live_mu.lock();
    defer st.live_mu.unlock();
    const gop = st.outputs.getOrPut(gpa, model) catch return;
    if (!gop.found_existing) {
        gop.key_ptr.* = gpa.dupe(u8, model) catch return;
        gop.value_ptr.* = .empty;
    }
    if (gop.value_ptr.items.len >= 200) _ = gop.value_ptr.orderedRemove(0);
    gop.value_ptr.append(gpa, out) catch {};
}
var state: ?*State = null;

fn loadState(path: []const u8) !*State {
    const bytes = try std.fs.cwd().readFileAlloc(gpa, path, 1 << 20);
    const parsed = try std.json.parseFromSlice(spend.Config, gpa, bytes, .{ .ignore_unknown_fields = true, .allocate = .alloc_always });
    const st = try gpa.create(State);
    st.* = .{ .cfg = parsed.value, .book = .{ .gpa = gpa }, .table = .{ .gpa = gpa, .path = parsed.value.prices } };
    st.book.policy = std.meta.stringToEnum(budget.Policy, parsed.value.policy) orelse return error.UnknownPolicy;
    const n = st.cfg.computers.len;
    st.covers = try gpa.alloc([2]usize, n);
    st.covers_len = try gpa.alloc(usize, n);
    st.budgeted = try gpa.alloc(bool, n);
    for (st.cfg.people) |p| _ = try st.book.add(try std.fmt.allocPrint(gpa, "person:{s}", .{p.name}), p.limits);
    for (st.cfg.computers, 0..) |cp, i| {
        const own = try st.book.add(cp.name, cp.limits);
        st.covers[i][0] = own;
        st.covers_len[i] = 1;
        st.budgeted[i] = hasLimits(cp.limits);
        if (cp.person) |pn| {
            for (st.cfg.people, 0..) |p, k| if (std.mem.eql(u8, p.name, pn)) {
                st.covers[i][1] = k;
                st.covers_len[i] = 2;
                if (hasLimits(p.limits)) st.budgeted[i] = true;
            };
        }
    }
    st.table.refresh();
    if (st.table.parsed == null) return error.NoPriceTable;
    return st;
}

fn hasLimits(l: budget.Limits) bool {
    return l.total != null or l.rate != null or l.plan_5h_max != null or l.plan_7d_max != null;
}

fn computerOf(fd: posix.fd_t) ?usize {
    const st = state orelse return null;
    var addr: posix.sockaddr.in = undefined;
    var len: posix.socklen_t = @sizeOf(posix.sockaddr.in);
    posix.getsockname(fd, @ptrCast(&addr), &len) catch return null;
    const b: [4]u8 = @bitCast(addr.addr);
    var buf: [16]u8 = undefined;
    const ip = std.fmt.bufPrint(&buf, "{d}.{d}.{d}.{d}", .{ b[0], b[1], b[2], b[3] }) catch return null;
    for (st.cfg.computers, 0..) |cp, i| if (std.mem.eql(u8, cp.address, ip)) return i;
    return null;
}

/// One model call the boundary admitted, from the moment it leaves until it is settled.
const Call = struct {
    id: u64,
    conn: u32,
    computer: usize,
    which: []const usize,
    format: usage.Format,
    req: spend.Request,
    row: ?prices.Row,
    cred_kind: []const u8,
    account: ?[]const u8,
    est_tokens: i64,
    est_from: []const u8,
    input_cost: f64,
    reserve: f64,
    meter: usage.Meter,
    body: std.ArrayList(u8) = .empty,
    status: u16 = 0,
    t0: f64,
    last_events: u32 = 0,
    settled: bool = false,

    /// What the call has cost so far, as the boundary sees it while it runs: its input (the
    /// provider's count once given, else the estimate), and its visible output at four
    /// characters a token.
    fn counted(self: *const Call) f64 {
        const r = self.row orelse return 0;
        var k = self.meter.counts;
        if (!self.meter.input_known) k = .{ .input = self.est_tokens };
        k.output = @max(k.output, @as(i64, @intCast(self.meter.visible_chars / 4)));
        const tps = state.?.cfg.output_tps;
        if (tps > 0) {
            const by_time: i64 = @intFromFloat((now_ms() - self.t0) / 1000 * tps);
            k.output = @max(k.output, @min(by_time, self.req.max_tokens));
        }
        return prices.cost(r, k);
    }

    fn progress(self: *Call) void {
        if (self.meter.events == self.last_events) return;
        self.last_events = self.meter.events;
        state.?.book.progress(self.which, self.id, self.counted());
    }

    /// Brings the count up to now even with no new event: hidden reasoning sends none.
    fn refresh(self: *Call) void {
        state.?.book.progress(self.which, self.id, self.counted());
    }

    fn deinit(self: *Call) void {
        self.meter.deinit();
        self.req.deinit(gpa);
        self.body.deinit(gpa);
    }
};

/// The sentence the program shows, in the provider's error shape.
fn refusalMessage(r: budget.Refusal, buf: []u8) []const u8 {
    const who = if (std.mem.startsWith(u8, r.ledger, "person:")) r.ledger[7..] else "this computer";
    return switch (r.why) {
        .total => std.fmt.bufPrint(buf, "Silo: {s}'s budget of ${d:.2} is reached (${d:.2} spent). Raise it in the Silo app to go on.", .{ who, r.limit, r.counted }) catch "Silo: the budget is reached.",
        .input => std.fmt.bufPrint(buf, "Silo: this request's input alone costs up to ${d:.2}, and {s}'s budget of ${d:.2} has ${d:.2} left. Raise it in the Silo app to go on.", .{ r.needed, who, r.limit, @max(0, r.limit - r.counted) }) catch "Silo: the call does not fit in the budget.",
        .held => std.fmt.bufPrint(buf, "Silo: the model calls under way hold what is left of {s}'s budget of ${d:.2}. Raise it in the Silo app to go on.", .{ who, r.limit }) catch "Silo: the budget is held.",
        .rate => blk: {
            const wait: f64 = if (r.passes_again_at) |t| @max(0, t - now_s()) else 0;
            break :blk std.fmt.bufPrint(buf, "Silo: {s}'s limit of ${d:.2} in the window is reached. Model calls pass again in {d:.0} s, or raise it in the Silo app.", .{ who, r.limit, wait }) catch "Silo: the rate is reached.";
        },
        .plan_5h => std.fmt.bufPrint(buf, "Silo: {s} may bring the plan's 5-hour window to {d:.0}%, and it is at {d:.0}%. Raise it in the Silo app to go on.", .{ who, r.limit * 100, r.counted * 100 }) catch "Silo: the plan share is reached.",
        .plan_7d => std.fmt.bufPrint(buf, "Silo: {s} may bring the plan's weekly window to {d:.0}%, and it is at {d:.0}%. Raise it in the Silo app to go on.", .{ who, r.limit * 100, r.counted * 100 }) catch "Silo: the plan share is reached.",
    };
}

fn now_s() f64 {
    return now_ms() / 1000;
}

const Begin = union(enum) { pass, refuse: []const u8, wait: struct { msg: []const u8, secs: u64 }, call: *Call };

/// Decides whether a model request leaves, before a byte of it does.
fn beginCall(st: *State, comp: usize, conn: u32, format: usage.Format, path: []const u8, head: []const u8, body: []const u8, msg_buf: []u8) !Begin {
    const which = st.covers[comp][0..st.covers_len[comp]];
    var req = spend.readRequest(gpa, format, path, body) catch {
        record("\"conn\":{d},\"kind\":\"unreadable-request\"", .{conn});
        if (st.budgeted[comp]) return .{ .refuse = "Silo: this model request cannot be read, and a budget covers this computer." };
        return .pass;
    };
    errdefer req.deinit(gpa);
    var day_buf: [10]u8 = undefined;
    const day = prices.today(&day_buf);
    const provider = spend.providerKey(format);
    const row = st.table.find(provider, req.model, req.tier, day);
    if (row == null and st.budgeted[comp]) {
        record("\"conn\":{d},\"kind\":\"refused\",\"why\":\"no-price\",\"model\":{f},\"tier\":{f}", .{ conn, j(req.model), j(req.tier) });
        const m = std.fmt.bufPrint(msg_buf, "Silo: there is no price for {s} ({s}) yet, and a budget covers this computer, so its calls cannot be counted.", .{ req.model, req.tier }) catch "Silo: no price for this model.";
        req.deinit(gpa);
        return .{ .refuse = m };
    }
    // The credential's kind and the plan account it names, if any.
    var kind: []const u8 = "unknown";
    var account: ?[]const u8 = null;
    if (spend.credentialOf(head, path)) |cv| {
        for (st.cfg.computers[comp].credentials) |cr| if (std.mem.eql(u8, cr.value, cv)) {
            kind = cr.kind;
            if (std.mem.eql(u8, cr.kind, "plan")) account = cr.value;
        };
    }
    // The input, estimated before it leaves: from the conversation's last reply when there is
    // one, else from the body's size.
    var est: i64 = 0;
    var continued: i64 = 0;
    var from: []const u8 = "cold";
    {
        st.memory_mu.lock();
        defer st.memory_mu.unlock();
        // The longest earlier request this one continues.
        var k = req.prefixes.len;
        while (k > 0) : (k -= 1) {
            const m = st.memory.get(req.prefixes[k - 1]) orelse continue;
            if (m.tokens <= 0) continue;
            const bpt = std.math.clamp(@as(f64, @floatFromInt(m.bytes)) / @as(f64, @floatFromInt(m.tokens)), 1.5, 8.0);
            const grown: f64 = @floatFromInt(@as(i64, @intCast(req.body_len)) - @as(i64, @intCast(m.bytes)));
            est = m.tokens + @as(i64, @intFromFloat(@max(0, grown) / bpt));
            continued = m.tokens;
            from = "conversation";
            break;
        }
    }
    if (std.mem.eql(u8, from, "cold")) est = @intFromFloat(@as(f64, @floatFromInt(req.body_len)) / st.cfg.cold_bytes_per_token);
    var input_cost: f64 = 0;
    var reserve: f64 = 0;
    if (st.cfg.output_tps > 0) {
        st.live_mu.lock();
        defer st.live_mu.unlock();
        for (st.live.items) |lc| lc.refresh();
    }
    if (row) |r| {
        input_cost = @as(f64, @floatFromInt(est)) * prices.dearestInput(r, est) / 1e6;
        if (std.mem.eql(u8, st.cfg.input_price, "likely") and continued > 0) {
            const fresh = est - continued;
            input_cost = (@as(f64, @floatFromInt(continued)) * r.cache_read + @as(f64, @floatFromInt(fresh)) * prices.dearestInput(r, est)) / 1e6;
        }
        const out_held: i64 = if (st.book.policy == .reserve_typical) @min(req.max_tokens, typicalOutput(st, req.model)) else req.max_tokens;
        reserve = input_cost + @as(f64, @floatFromInt(out_held)) * prices.outputRate(r, est) / 1e6;
    }
    if (st.book.admit(which, input_cost, reserve, account, now_s())) |r| {
        record("\"conn\":{d},\"kind\":\"refused\",\"why\":\"{s}\",\"ledger\":{f},\"limit\":{d:.4},\"counted\":{d:.4},\"input_cost\":{d:.4},\"est_tokens\":{d},\"model\":{f}", .{ conn, @tagName(r.why), j(r.ledger), r.limit, r.counted, input_cost, est, j(req.model) });
        req.deinit(gpa);
        if (r.why == .rate and std.mem.eql(u8, st.cfg.rate_refusal, "429")) {
            const secs: u64 = if (r.passes_again_at) |t| @intFromFloat(@ceil(@max(1, t - now_s()))) else 60;
            return .{ .wait = .{ .msg = refusalMessage(r, msg_buf), .secs = secs } };
        }
        return .{ .refuse = refusalMessage(r, msg_buf) };
    }
    const call = try gpa.create(Call);
    call.* = .{
        .id = st.next_call.fetchAdd(1, .monotonic),
        .conn = conn,
        .computer = comp,
        .which = which,
        .format = format,
        .req = req,
        .row = row,
        .cred_kind = kind,
        .account = account,
        .est_tokens = est,
        .est_from = from,
        .input_cost = input_cost,
        .reserve = reserve,
        .meter = usage.Meter.init(gpa, format),
        .t0 = now_ms(),
    };
    if (format == .gemini) call.meter.setModel(req.model);
    try st.book.start(which, call.id, reserve);
    {
        st.live_mu.lock();
        defer st.live_mu.unlock();
        try st.live.append(gpa, call);
    }
    return .{ .call = call };
}

/// The call ended, whole or not: settle it on the provider's count, or on an estimate.
fn endCall(st: *State, call: *Call, how: []const u8) void {
    if (call.settled) return;
    call.settled = true;
    {
        st.live_mu.lock();
        defer st.live_mu.unlock();
        for (st.live.items, 0..) |lc, i| if (lc == call) {
            _ = st.live.swapRemove(i);
            break;
        };
    }
    if (call.body.items.len > 0) call.meter.onBody(call.body.items) catch {};
    var day_buf: [10]u8 = undefined;
    const day = prices.today(&day_buf);
    const provider = spend.providerKey(call.format);
    var cost: f64 = 0;
    var estimated = false;
    var priced = true;
    const billed = call.meter.billed();
    if (call.status != 0 and call.status != 200 and !call.meter.final) {
        // An error reply: nothing generated, nothing billed.
        cost = 0;
    } else if (call.meter.final) {
        if (spend.costOf(&st.table, provider, &call.meter, day)) |v| cost = v else {
            priced = false;
            if (call.row) |r| cost = prices.cost(r, billed);
        }
    } else if (call.row) |r| {
        // No final count: the input as counted or estimated, and the visible output, at four
        // characters a token. Hidden reasoning is missed: this is a floor.
        var c2 = call.meter.counts;
        if (!call.meter.input_known) c2 = .{ .input = call.est_tokens };
        c2.output = @max(c2.output, @as(i64, @intCast(call.meter.visible_chars / 4)));
        cost = prices.cost(r, c2);
        estimated = true;
    }
    const s = st.book.settle(call.which, call.id, cost, estimated, now_s()) catch budget.Book.Settled{};
    if (call.meter.final) noteOutput(st, call.req.model, billed.output);
    // The conversation's size is the top-level input: attempts inside one call (a fallback) each
    // read the same conversation, so their sum is not its size.
    const context = call.meter.counts.totalInput();
    if (call.meter.final and context > 0) {
        st.memory_mu.lock();
        defer st.memory_mu.unlock();
        if (st.memory.count() > 1 << 16) st.memory.clearRetainingCapacity();
        st.memory.put(gpa, call.req.whole(), .{ .bytes = call.req.body_len, .tokens = context }) catch {};
    }
    const l0 = &st.book.ledgers.items[call.which[0]];
    const lp: ?*budget.Ledger = if (call.which.len > 1) &st.book.ledgers.items[call.which[1]] else null;
    record("\"conn\":{d},\"kind\":\"call\",\"call\":{d},\"computer\":{f},\"format\":\"{s}\",\"model\":{f},\"reply_model\":{f},\"tier\":{f},\"credential\":{f},\"status\":{d},\"end\":{f},\"final\":{},\"estimated\":{},\"priced\":{},\"est_tokens\":{d},\"context\":{d},\"est_from\":\"{s}\",\"input_cost_bound\":{d:.6},\"reserve\":{d:.6},\"input\":{d},\"cache_read\":{d},\"cache_write_short\":{d},\"cache_write_long\":{d},\"output\":{d},\"reasoning\":{d},\"parts\":{d},\"visible_chars\":{d},\"cost\":{d:.6},\"max_tokens\":{d},\"ms\":{d:.1}", .{
            call.conn,                          call.id,                         j(st.cfg.computers[call.computer].name), @tagName(call.format), j(call.req.model),
            j(call.meter.model orelse ""),      j(call.meter.tier orelse call.req.tier), j(call.cred_kind),             call.status,          j(how),
            call.meter.final,                   estimated,                       priced,                                  call.est_tokens,      context,              call.est_from,
            call.input_cost,                    call.reserve,                    billed.input,                            billed.cache_read,    billed.cache_write_short,
            billed.cache_write_long,            billed.output,                   billed.reasoning,                        call.meter.parts.items.len, call.meter.visible_chars,
            cost,                               call.req.max_tokens,             now_ms() - call.t0,
        });
    record("\"conn\":{d},\"kind\":\"ledgers\",\"call\":{d},\"computer_spent\":{d:.6},\"person_spent\":{d:.6},\"plan_5h_moved\":{d:.4},\"plan_7d_moved\":{d:.4}", .{ call.conn, call.id, l0.settled, if (lp) |p| p.settled else 0, l0.plan_5h_moved, l0.plan_7d_moved });
    if (s.alert) |n| record("\"kind\":\"alert\",\"ledger\":{f},\"spent\":{d:.4}", .{ j(n), l0.settled });
    if (s.reached) |n| record("\"kind\":\"reached\",\"ledger\":{f}", .{j(n)});
}

// ---- the probe ----
var probe_mu: std.Thread.Mutex = .{};
var probe_ctl_fd: posix.fd_t = -1;
var probe_ack_fd: posix.fd_t = -1;
var pending: std.StringHashMapUnmanaged(void) = .empty;

/// Tells the probe to arm or disarm, and waits for its answer, up to a limit.
fn probeCommand(cmd: []const u8) !f64 {
    if (probe_ctl_fd < 0) return 0;
    const t0 = now_ms();
    var line_buf: [32]u8 = undefined;
    const line = try std.fmt.bufPrint(&line_buf, "{s}\n", .{cmd});
    _ = try posix.write(probe_ctl_fd, line);
    var got: [64]u8 = undefined;
    var n: usize = 0;
    const deadline = t0 + 2000;
    while (now_ms() < deadline) {
        var fds = [_]posix.pollfd{.{ .fd = probe_ack_fd, .events = posix.POLL.IN, .revents = 0 }};
        if (try posix.poll(&fds, 50) == 0) continue;
        const r = try posix.read(probe_ack_fd, got[n..]);
        n += r;
        if (std.mem.indexOfScalar(u8, got[0..n], '\n') != null) {
            if (std.mem.indexOf(u8, got[0..n], cmd) == null) return error.ProbeAnsweredWrong;
            return now_ms() - t0;
        }
    }
    return error.ProbeDidNotAnswer;
}

// ---- TLS ----
fn alpnSelect(_: ?*c.SSL, out: [*c][*c]const u8, outlen: [*c]u8, in: [*c]const u8, inlen: c_uint, _: ?*anyopaque) callconv(.c) c_int {
    var i: usize = 0;
    while (i < inlen) {
        const l = in[i];
        if (i + 1 + l > inlen) break;
        if (std.mem.eql(u8, in[i + 1 .. i + 1 + l], "http/1.1")) {
            out.* = in + i + 1;
            outlen.* = l;
            return c.SSL_TLSEXT_ERR_OK;
        }
        i += 1 + l;
    }
    return c.SSL_TLSEXT_ERR_NOACK;
}

fn sslErr(what: []const u8) error{Tls} {
    var buf: [256]u8 = undefined;
    const e = c.ERR_get_error();
    c.ERR_error_string_n(e, &buf, buf.len);
    std.log.warn("{s}: {s}", .{ what, std.mem.sliceTo(&buf, 0) });
    return error.Tls;
}

const Side = struct {
    ssl: *c.SSL,
    fd: posix.fd_t,
    fn read(self: Side, buf: []u8) !usize {
        const r = c.SSL_read(self.ssl, buf.ptr, @intCast(buf.len));
        if (r > 0) return @intCast(r);
        const e = c.SSL_get_error(self.ssl, r);
        if (e == c.SSL_ERROR_ZERO_RETURN or e == c.SSL_ERROR_SYSCALL) return 0;
        return sslErr("SSL_read");
    }
    fn writeAll(self: Side, buf: []const u8) !void {
        var off: usize = 0;
        while (off < buf.len) {
            const r = c.SSL_write(self.ssl, buf.ptr + off, @intCast(buf.len - off));
            if (r <= 0) return sslErr("SSL_write");
            off += @intCast(r);
        }
    }
    fn close(self: Side) void {
        _ = c.SSL_shutdown(self.ssl);
        c.SSL_free(self.ssl);
        posix.close(self.fd);
    }
};

// ---- HTTP ----
const Head = struct {
    raw: []u8, // the head as it came, up to and including the blank line
    first: []const u8,
    content_length: ?u64 = null,
    chunked: bool = false,
    encoding: enum { identity, gzip, zstd, other } = .identity,
    event_stream: bool = false,
    upgrade: bool = false,
    ws_deflate: bool = false,
    ws_server_nctx: bool = false,
    ws_client_nctx: bool = false,
    host: []const u8 = "",
    status: u16 = 0,
};

/// Reads until the end of a head. Bytes read past it stay in `extra`.
fn readHead(side: Side, extra: *std.ArrayList(u8)) !?Head {
    var buf: [16384]u8 = undefined;
    while (std.mem.indexOf(u8, extra.items, "\r\n\r\n") == null) {
        if (extra.items.len > 1 << 20) return error.HeadTooLarge;
        const n = try side.read(&buf);
        if (n == 0) return null;
        try extra.appendSlice(gpa, buf[0..n]);
    }
    const end = std.mem.indexOf(u8, extra.items, "\r\n\r\n").? + 4;
    const raw = try gpa.dupe(u8, extra.items[0..end]);
    std.mem.copyForwards(u8, extra.items, extra.items[end..]);
    extra.shrinkRetainingCapacity(extra.items.len - end);
    var h: Head = .{ .raw = raw, .first = "" };
    var it = std.mem.splitSequence(u8, raw[0 .. raw.len - 4], "\r\n");
    h.first = it.next() orelse return error.BadHead;
    while (it.next()) |line| {
        const colon = std.mem.indexOfScalar(u8, line, ':') orelse continue;
        const name = std.mem.trim(u8, line[0..colon], " ");
        const value = std.mem.trim(u8, line[colon + 1 ..], " \t");
        if (std.ascii.eqlIgnoreCase(name, "content-length")) h.content_length = std.fmt.parseInt(u64, value, 10) catch return error.BadHead;
        if (std.ascii.eqlIgnoreCase(name, "transfer-encoding") and std.ascii.indexOfIgnoreCase(value, "chunked") != null) h.chunked = true;
        if (std.ascii.eqlIgnoreCase(name, "content-encoding")) {
            h.encoding = if (std.ascii.eqlIgnoreCase(value, "gzip")) .gzip else if (std.ascii.eqlIgnoreCase(value, "zstd")) .zstd else if (std.ascii.eqlIgnoreCase(value, "identity")) .identity else .other;
        }
        if (std.ascii.eqlIgnoreCase(name, "content-type") and std.ascii.indexOfIgnoreCase(value, "text/event-stream") != null) h.event_stream = true;
        if (std.ascii.eqlIgnoreCase(name, "upgrade")) h.upgrade = true;
        if (std.ascii.eqlIgnoreCase(name, "sec-websocket-extensions") and std.ascii.indexOfIgnoreCase(value, "permessage-deflate") != null) {
            h.ws_deflate = true;
            h.ws_server_nctx = std.ascii.indexOfIgnoreCase(value, "server_no_context_takeover") != null;
            h.ws_client_nctx = std.ascii.indexOfIgnoreCase(value, "client_no_context_takeover") != null;
        }
        if (std.ascii.eqlIgnoreCase(name, "host")) h.host = value;
    }
    if (std.mem.startsWith(u8, h.first, "HTTP/")) {
        var parts = std.mem.splitScalar(u8, h.first, ' ');
        _ = parts.next();
        h.status = std.fmt.parseInt(u16, parts.next() orelse "0", 10) catch 0;
    }
    return h;
}

/// Turns the body's bytes, after chunked framing, into what the provider meant.
const Decoder = struct {
    kind: @TypeOf(@as(Head, undefined).encoding),
    z: c.z_stream = std.mem.zeroes(c.z_stream),
    zs: ?*c.ZSTD_DStream = null,
    /// In place: zlib keeps a pointer back to its stream.
    fn init(d: *Decoder, kind: @TypeOf(@as(Head, undefined).encoding)) !void {
        d.* = .{ .kind = kind };
        switch (kind) {
            .gzip => if (c.inflateInit2_(&d.z, 47, c.zlibVersion(), @sizeOf(c.z_stream)) != c.Z_OK) return error.Inflate,
            .zstd => {
                d.zs = c.ZSTD_createDStream();
                _ = c.ZSTD_initDStream(d.zs);
            },
            else => {},
        }
    }
    fn deinit(d: *Decoder) void {
        switch (d.kind) {
            .gzip => _ = c.inflateEnd(&d.z),
            .zstd => _ = c.ZSTD_freeDStream(d.zs),
            else => {},
        }
    }
    fn feed(d: *Decoder, in: []const u8, out: *std.ArrayList(u8)) !void {
        var tmp: [65536]u8 = undefined;
        switch (d.kind) {
            .identity, .other => try out.appendSlice(gpa, in),
            .gzip => {
                d.z.next_in = @constCast(in.ptr);
                d.z.avail_in = @intCast(in.len);
                while (d.z.avail_in > 0) {
                    d.z.next_out = &tmp;
                    d.z.avail_out = tmp.len;
                    const r = c.inflate(&d.z, c.Z_NO_FLUSH);
                    if (r != c.Z_OK and r != c.Z_STREAM_END and r != c.Z_BUF_ERROR) return error.Inflate;
                    try out.appendSlice(gpa, tmp[0 .. tmp.len - d.z.avail_out]);
                    if (r == c.Z_STREAM_END) break;
                }
            },
            .zstd => {
                var ib: c.ZSTD_inBuffer = .{ .src = in.ptr, .size = in.len, .pos = 0 };
                while (ib.pos < ib.size) {
                    var ob: c.ZSTD_outBuffer = .{ .dst = &tmp, .size = tmp.len, .pos = 0 };
                    const r = c.ZSTD_decompressStream(d.zs, &ob, &ib);
                    if (c.ZSTD_isError(r) != 0) return error.Zstd;
                    try out.appendSlice(gpa, tmp[0..ob.pos]);
                }
            },
        }
    }
};

/// Reads one body from `from` as framed by `h`, calling `onRaw` with each raw piece and the
/// decoded copy. Returns false when the body ended with the connection.
fn pumpBody(from: Side, h: *const Head, extra: *std.ArrayList(u8), ctx: anytype) !bool {
    var ch: stream.Chunked = .{};
    var dec: Decoder = undefined;
    try dec.init(h.encoding);
    defer dec.deinit();
    var framed: std.ArrayList(u8) = .empty;
    defer framed.deinit(gpa);
    var decoded: std.ArrayList(u8) = .empty;
    defer decoded.deinit(gpa);
    var left: ?u64 = if (h.chunked) null else h.content_length;
    const until_close = !h.chunked and h.content_length == null;
    if (!h.chunked and left != null and left.? == 0) return true;
    if (!h.chunked and h.content_length == null and (h.status == 204 or h.status == 304 or (h.status >= 100 and h.status < 200))) return true;
    if (!until_close and !h.chunked and std.mem.startsWith(u8, h.first, "HTTP/") == false and h.content_length == null) return true;
    var buf: [65536]u8 = undefined;
    var piece: []const u8 = extra.items;
    var from_extra = true;
    while (true) {
        if (piece.len == 0) {
            const n = try from.read(&buf);
            if (n == 0) {
                // The body ended with the connection. The last piece was already handed over.
                decoded.clearRetainingCapacity();
                try ctx.onRaw("", &decoded, true);
                return false;
            }
            piece = buf[0..n];
            from_extra = false;
        }
        var take = piece;
        if (left) |l| take = piece[0..@intCast(@min(l, piece.len))];
        framed.clearRetainingCapacity();
        decoded.clearRetainingCapacity();
        if (h.chunked) {
            // Find where the chunked body ends inside this piece, byte by byte at the end.
            var k: usize = 0;
            while (k < take.len and !ch.finished()) : (k += 1) try ch.feed(gpa, take[k .. k + 1], &framed);
            take = take[0..k];
        } else try framed.appendSlice(gpa, take);
        try dec.feed(framed.items, &decoded);
        const finished = (h.chunked and ch.finished()) or (left != null and left.? == take.len);
        if (left) |*l| l.* -= take.len;
        try ctx.onRaw(take, &decoded, finished);
        const rest = piece[take.len..];
        if (from_extra) {
            const keep = try gpa.dupe(u8, rest);
            defer gpa.free(keep);
            extra.clearRetainingCapacity();
            try extra.appendSlice(gpa, keep);
            piece = extra.items;
            if (finished) return true;
            extra.clearRetainingCapacity();
            piece = "";
        } else {
            if (finished) {
                try extra.appendSlice(gpa, rest);
                return true;
            }
            piece = "";
        }
    }
}

const ReqCtx = struct {
    body: std.ArrayList(u8) = .empty,
    raw: std.ArrayList(u8) = .empty,
    pub fn onRaw(self: *ReqCtx, raw: []const u8, decoded: *std.ArrayList(u8), _: bool) !void {
        try self.raw.appendSlice(gpa, raw);
        try self.body.appendSlice(gpa, decoded.items);
    }
};

const RespCtx = struct {
    to: Side,
    provider: stream.Provider,
    conn: u32,
    watcher: stream.Watcher,
    sse: stream.Sse = .{},
    event_stream: bool,
    held: std.ArrayList(u8) = .empty,
    hold_start: f64 = 0,
    decided: usize = 0,
    bytes: u64 = 0,
    first_byte: f64 = 0,
    raw_hash: std.crypto.hash.sha2.Sha256 = std.crypto.hash.sha2.Sha256.init(.{}),
    holds: u32 = 0,
    held_ms_total: f64 = 0,
    messages: u32 = 0,
    call: ?*Call = null,

    /// Each event goes to the tool-call watcher and to the meter.
    pub fn onEvent(self: *RespCtx, ev: stream.Sse.Event) !void {
        try self.watcher.onEvent(ev);
        if (self.call) |cl| {
            try cl.meter.onEvent(ev);
            cl.progress();
        }
    }

    pub fn onRaw(self: *RespCtx, raw: []const u8, decoded: *std.ArrayList(u8), finished: bool) !void {
        if (raw.len > 0 and self.first_byte == 0) self.first_byte = now_ms();
        self.bytes += raw.len;
        self.raw_hash.update(raw);
        if (self.event_stream and decoded.items.len > 0) {
            self.sse.feed(gpa, decoded.items, self) catch |e| {
                record("\"conn\":{d},\"kind\":\"unreadable\",\"error\":{f}", .{ self.conn, j(@errorName(e)) });
            };
        } else if (self.call) |cl| {
            if (cl.body.items.len < 64 << 20) try cl.body.appendSlice(gpa, decoded.items);
        }
        try self.release(raw, finished);
    }

    /// A WebSocket message from the host is one event of the reply.
    pub fn onMessage(self: *RespCtx, m: ws.Message) !void {
        self.messages += 1;
        if (m.opcode == 1) try self.onEvent(.{ .name = "", .data = m.data });
    }

    /// Puts each call that became whole to the precaution, then holds or forwards `raw`.
    fn release(self: *RespCtx, raw: []const u8, finished: bool) !void {
        // Each call that became whole is put to the precaution, in order.
        while (self.decided < self.watcher.done.items.len) : (self.decided += 1) {
            const t = self.watcher.done.items[self.decided];
            const deny = if (cfg.deny_substr) |d| std.mem.indexOf(u8, t.input, d) != null else false;
            var ack_ms: f64 = 0;
            var outcome: []const u8 = if (deny) "deny" else "allow";
            if (deny) {
                probe_mu.lock();
                defer probe_mu.unlock();
                ack_ms = probeCommand("arm") catch |e| blk: {
                    // The probe did not answer: the reply does not go on.
                    record("\"conn\":{d},\"kind\":\"probe-failed\",\"error\":{f}", .{ self.conn, j(@errorName(e)) });
                    outcome = "withheld";
                    break :blk -1;
                };
                if (ack_ms >= 0) try pending.put(gpa, try gpa.dupe(u8, t.id), {});
            }
            record("\"conn\":{d},\"kind\":\"tool-call\",\"id\":{f},\"name\":{f},\"input_sha256\":\"{s}\",\"input_bytes\":{d},\"outcome\":\"{s}\",\"probe_ms\":{d:.2}", .{ self.conn, j(t.id), j(t.name), sha(t.input), t.input.len, outcome, ack_ms });
            if (std.mem.eql(u8, outcome, "withheld")) return error.Withheld;
        }
        const holding = cfg.hold and self.watcher.open > 0;
        if (holding) {
            if (self.held.items.len == 0) {
                self.hold_start = now_ms();
                self.holds += 1;
            }
            try self.held.appendSlice(gpa, raw);
        } else {
            if (self.held.items.len > 0) {
                try self.to.writeAll(self.held.items);
                self.held.clearRetainingCapacity();
                self.held_ms_total += now_ms() - self.hold_start;
            }
            if (raw.len > 0) try self.to.writeAll(raw);
        }
        if (finished and self.held.items.len > 0) {
            // A reply that ends inside a tool call: what was held goes on as it is.
            try self.to.writeAll(self.held.items);
            self.held.clearRetainingCapacity();
        }
    }
};

/// The program answered calls Silo denied: once none is pending, disarm, and wait for the probe.
fn noteResults(results: []const []u8, id: u32) void {
    probe_mu.lock();
    defer probe_mu.unlock();
    var answered = false;
    for (results) |r| {
        if (pending.fetchRemove(r)) |kv| {
            gpa.free(kv.key);
            answered = true;
        }
    }
    if (answered and pending.count() == 0) {
        const ms = probeCommand("disarm") catch -1;
        record("\"conn\":{d},\"kind\":\"probe-disarmed\",\"probe_ms\":{d:.2}", .{ id, ms });
    }
}

const ClientWs = struct {
    id: u32,
    messages: u32 = 0,
    pub fn onMessage(self: *ClientWs, m: ws.Message) !void {
        self.messages += 1;
        if (m.opcode != 1) return;
        var results = try stream.toolResultIds(gpa, .openai, m.data);
        defer {
            for (results.items) |x| gpa.free(x);
            results.deinit(gpa);
        }
        var prev: []const u8 = "";
        var typ: []const u8 = "";
        var parsed = std.json.parseFromSlice(std.json.Value, gpa, m.data, .{}) catch null;
        defer if (parsed) |*p| p.deinit();
        if (parsed) |p| if (p.value == .object) {
            if (p.value.object.get("previous_response_id")) |v| if (v == .string) {
                prev = v.string;
            };
            if (p.value.object.get("type")) |v| if (v == .string) {
                typ = v.string;
            };
        };
        record("\"conn\":{d},\"kind\":\"ws-request\",\"type\":{f},\"bytes\":{d},\"previous_response_id\":{f},\"results\":{d}", .{ self.id, j(typ), m.data.len, j(prev), results.items.len });
        noteResults(results.items, self.id);
    }
};

/// After a 101: both directions, read and forwarded as they come, in one thread.
fn wsRelay(client: Side, up: Side, provider: stream.Provider, id: u32, head: *const Head, cextra: []const u8, uextra: []const u8) !void {
    var cr: ws.Reader = .{ .gpa = gpa, .deflate = head.ws_deflate, .no_context_takeover = head.ws_client_nctx };
    defer cr.deinit();
    var hr: ws.Reader = .{ .gpa = gpa, .deflate = head.ws_deflate, .no_context_takeover = head.ws_server_nctx };
    defer hr.deinit();
    var csink: ClientWs = .{ .id = id };
    var ctx: RespCtx = .{ .to = client, .provider = provider, .conn = id, .watcher = stream.Watcher.init(gpa, provider), .event_stream = false };
    defer ctx.watcher.deinit();
    defer ctx.held.deinit(gpa);
    record("\"conn\":{d},\"kind\":\"websocket\",\"deflate\":{}", .{ id, head.ws_deflate });
    if (cextra.len > 0) {
        try cr.feed(cextra, &csink);
        try up.writeAll(cextra);
    }
    if (uextra.len > 0) {
        try hr.feed(uextra, &ctx);
        try ctx.release(uextra, false);
    }
    var buf: [65536]u8 = undefined;
    while (true) {
        if (c.SSL_pending(client.ssl) == 0 and c.SSL_pending(up.ssl) == 0) {
            var fds = [_]posix.pollfd{ .{ .fd = client.fd, .events = posix.POLL.IN, .revents = 0 }, .{ .fd = up.fd, .events = posix.POLL.IN, .revents = 0 } };
            _ = try posix.poll(&fds, -1);
            if (fds[0].revents != 0) {
                const n = try client.read(&buf);
                if (n == 0) break;
                try cr.feed(buf[0..n], &csink);
                try up.writeAll(buf[0..n]);
            }
            if (fds[1].revents != 0) {
                const n = try up.read(&buf);
                if (n == 0) break;
                try hr.feed(buf[0..n], &ctx);
                try ctx.release(buf[0..n], false);
            }
            continue;
        }
        if (c.SSL_pending(client.ssl) > 0) {
            const n = try client.read(&buf);
            if (n == 0) break;
            try cr.feed(buf[0..n], &csink);
            try up.writeAll(buf[0..n]);
        }
        if (c.SSL_pending(up.ssl) > 0) {
            const n = try up.read(&buf);
            if (n == 0) break;
            try hr.feed(buf[0..n], &ctx);
            try ctx.release(buf[0..n], false);
        }
    }
    if (ctx.held.items.len > 0) try client.writeAll(ctx.held.items);
    record("\"conn\":{d},\"kind\":\"websocket-closed\",\"from_program\":{d},\"from_host\":{d},\"tool_calls\":{d},\"holds\":{d},\"held_ms\":{d:.2},\"input_tokens\":{d},\"output_tokens\":{d}", .{ id, csink.messages, ctx.messages, ctx.watcher.done.items.len, ctx.holds, ctx.held_ms_total, ctx.watcher.input_tokens orelse -1, ctx.watcher.output_tokens orelse -1 });
}

fn watcherProvider(p: spend.Provider) stream.Provider {
    return switch (p) {
        .anthropic => .anthropic,
        .openai, .google => .openai,
    };
}

fn defaultFormat(p: ?spend.Provider) usage.Format {
    return switch (p orelse return .responses) {
        .anthropic => .anthropic,
        .openai => .responses,
        .google => .gemini,
    };
}

fn isUncountable(host: []const u8) bool {
    const st = state orelse return false;
    for (st.cfg.uncountable_hosts) |h| if (std.mem.eql(u8, h, host)) return true;
    return false;
}

fn handle(fd: posix.fd_t) void {
    const id = conn_counter.fetchAdd(1, .monotonic);
    handleInner(fd, id) catch |e| {
        record("\"conn\":{d},\"kind\":\"closed\",\"error\":{f}", .{ id, j(@errorName(e)) });
    };
}

fn noDelay(fd: posix.fd_t) void {
    const one: c_int = 1;
    posix.setsockopt(fd, posix.IPPROTO.TCP, posix.TCP.NODELAY, std.mem.asBytes(&one)) catch {};
}

fn writeRefusal(client: Side, format: usage.Format, msg: []const u8) !void {
    const r = try spend.refusal(gpa, format, msg);
    defer gpa.free(r);
    try client.writeAll(r);
}

/// A known model host Silo cannot count, while a budget covers the computer: every request is
/// refused, in OpenAI's shape, which most such hosts copy.
fn refuseAll(client: Side, id: u32, sni: []const u8) !void {
    var extra: std.ArrayList(u8) = .empty;
    defer extra.deinit(gpa);
    while (true) {
        var req = (try readHead(client, &extra)) orelse return;
        defer gpa.free(req.raw);
        var rc: ReqCtx = .{};
        defer rc.body.deinit(gpa);
        defer rc.raw.deinit(gpa);
        if (req.content_length != null or req.chunked) _ = try pumpBody(client, &req, &extra, &rc);
        var buf: [256]u8 = undefined;
        const m = std.fmt.bufPrint(&buf, "Silo: {s} is a model host Silo cannot count, and a budget covers this computer.", .{sni}) catch "Silo: uncountable host.";
        record("\"conn\":{d},\"kind\":\"refused\",\"why\":\"uncountable-host\",\"sni\":{f}", .{ id, j(sni) });
        try writeRefusal(client, .responses, m);
    }
}

fn handleInner(fd: posix.fd_t, id: u32) !void {
    noDelay(fd);
    const comp = computerOf(fd);
    const ssl = c.SSL_new(server_ctx) orelse return sslErr("SSL_new");
    _ = c.SSL_set_fd(ssl, fd);
    if (c.SSL_accept(ssl) != 1) {
        record("\"conn\":{d},\"kind\":\"handshake-failed\",\"side\":\"program\"", .{id});
        c.SSL_free(ssl);
        posix.close(fd);
        return;
    }
    const client: Side = .{ .ssl = ssl, .fd = fd };
    defer client.close();
    const sni_c = c.SSL_get_servername(ssl, c.TLSEXT_NAMETYPE_host_name);
    const sni: []const u8 = if (sni_c != null) std.mem.span(sni_c) else "";
    const provider = spend.providerOf(sni);
    if (provider == null) {
        if (isUncountable(sni) and comp != null and state.?.budgeted[comp.?]) return refuseAll(client, id, sni);
        if (!isUncountable(sni)) {
            record("\"conn\":{d},\"kind\":\"refused\",\"sni\":{f}", .{ id, j(sni) });
            return;
        }
    }
    // Silo's own connection to the host, checked against Silo's own store.
    const addr = try std.net.Address.parseIp4(cfg.upstream_ip, cfg.upstream_port);
    const up_stream = try std.net.tcpConnectToAddress(addr);
    noDelay(up_stream.handle);
    const ussl = c.SSL_new(client_ctx) orelse return sslErr("SSL_new");
    _ = c.SSL_set_fd(ussl, up_stream.handle);
    const sni_z = try gpa.dupeZ(u8, sni);
    defer gpa.free(sni_z);
    _ = c.SSL_ctrl(ussl, c.SSL_CTRL_SET_TLSEXT_HOSTNAME, c.TLSEXT_NAMETYPE_host_name, @ptrCast(@constCast(sni_z.ptr)));
    _ = c.SSL_set1_host(ussl, sni_z.ptr);
    _ = c.SSL_set_alpn_protos(ussl, "\x08http/1.1", 9);
    if (c.SSL_connect(ussl) != 1) {
        record("\"conn\":{d},\"kind\":\"handshake-failed\",\"side\":\"host\",\"sni\":{f},\"verify\":{d}", .{ id, j(sni), c.SSL_get_verify_result(ussl) });
        c.SSL_free(ussl);
        up_stream.close();
        return;
    }
    const up: Side = .{ .ssl = ussl, .fd = up_stream.handle };
    defer up.close();
    const comp_name: []const u8 = if (comp) |k| state.?.cfg.computers[k].name else "";
    record("\"conn\":{d},\"kind\":\"open\",\"sni\":{f},\"computer\":{f},\"tls\":{f}", .{ id, j(sni), j(comp_name), j(std.mem.span(c.SSL_get_version(ssl))) });

    var cextra: std.ArrayList(u8) = .empty;
    defer cextra.deinit(gpa);
    var uextra: std.ArrayList(u8) = .empty;
    defer uextra.deinit(gpa);
    while (true) {
        var req = (try readHead(client, &cextra)) orelse return;
        defer gpa.free(req.raw);
        const t_req = now_ms();
        var rc: ReqCtx = .{};
        defer rc.body.deinit(gpa);
        defer rc.raw.deinit(gpa);
        if (req.content_length != null or req.chunked) _ = try pumpBody(client, &req, &cextra, &rc);

        // ---- budgets: before a byte of the request leaves ----
        var call: ?*Call = null;
        var msg_buf: [512]u8 = undefined;
        var parts = std.mem.splitScalar(u8, req.first, ' ');
        const method = parts.next() orelse "";
        const path = parts.next() orelse "";
        if (state) |st| if (comp) |k| {
            const fmt = if (provider) |p| spend.formatOf(p, method, path) else null;
            // A credential this computer was not given does not leave a host Silo reads.
            if (st.cfg.deny_unknown_credentials and provider != null) if (spend.credentialOf(req.raw, path)) |cv| {
                var known = false;
                for (st.cfg.computers[k].credentials) |cr| known = known or std.mem.eql(u8, cr.value, cv);
                if (!known) {
                    record("\"conn\":{d},\"kind\":\"refused\",\"why\":\"unknown-credential\",\"line\":{f},\"credential_tail\":{f}", .{ id, j(req.first), j(cv[cv.len -| 4..]) });
                    try writeRefusal(client, fmt orelse defaultFormat(provider), "Silo: this computer was not given this key, so it cannot be used here. Add it to Silo to use it.");
                    continue;
                }
            };
            if (fmt) |f| switch (try beginCall(st, k, id, f, path, req.raw, rc.body.items, &msg_buf)) {
                .pass => {},
                .refuse => |m| {
                    try writeRefusal(client, f, m);
                    continue;
                },
                .wait => |wt| {
                    const r = try spend.wait(gpa, f, wt.msg, wt.secs);
                    defer gpa.free(r);
                    try client.writeAll(r);
                    continue;
                },
                .call => |cl| call = cl,
            };
        };
        defer if (call) |cl| {
            endCall(state.?, cl, "connection-ended");
            cl.deinit();
            gpa.destroy(cl);
        };

        // The program answered a call Silo denied: disarm first, and wait for the probe.
        var results = try stream.toolResultIds(gpa, watcherProvider(provider orelse .openai), rc.body.items);
        defer {
            for (results.items) |x| gpa.free(x);
            results.deinit(gpa);
        }
        noteResults(results.items, id);
        try up.writeAll(req.raw);
        try up.writeAll(rc.raw.items);
        const t_sent = now_ms();
        var full_hash: [64]u8 = undefined;
        {
            var hh = std.crypto.hash.sha2.Sha256.init(.{});
            hh.update(req.raw);
            hh.update(rc.raw.items);
            var d: [32]u8 = undefined;
            hh.final(&d);
            full_hash = std.fmt.bytesToHex(d, .lower);
        }
        record("\"conn\":{d},\"kind\":\"request\",\"line\":{f},\"bytes\":{d},\"body_bytes_decoded\":{d},\"raw_sha256\":\"{s}\",\"results\":{d},\"read_ms\":{d:.2}", .{ id, j(req.first), req.raw.len + rc.raw.items.len, rc.body.items.len, full_hash, results.items.len, t_sent - t_req });

        var resp = (try readHead(up, &uextra)) orelse return;
        defer gpa.free(resp.raw);
        const t_head = now_ms();
        if (call) |cl| {
            cl.status = resp.status;
            const pw = spend.planWindows(resp.raw);
            if (cl.account) |acct| if (pw.five_hour != null or pw.seven_day != null) {
                state.?.book.planReading(cl.which, acct, pw.five_hour, pw.seven_day, now_s()) catch {};
            };
        }
        try client.writeAll(resp.raw);
        if (resp.status == 101) return wsRelay(client, up, watcherProvider(provider orelse .openai), id, &resp, cextra.items, uextra.items);
        var ctx: RespCtx = .{ .to = client, .provider = watcherProvider(provider orelse .openai), .conn = id, .watcher = stream.Watcher.init(gpa, watcherProvider(provider orelse .openai)), .event_stream = resp.event_stream, .call = call };
        defer ctx.watcher.deinit();
        defer ctx.sse.deinit(gpa);
        defer ctx.held.deinit(gpa);
        ctx.raw_hash.update(resp.raw);
        const keep = try pumpBody(up, &resp, &uextra, &ctx);
        const t_end = now_ms();
        if (call) |cl| endCall(state.?, cl, if (keep) "whole" else "host-closed");
        var d: [32]u8 = undefined;
        ctx.raw_hash.final(&d);
        record("\"conn\":{d},\"kind\":\"reply\",\"status\":{d},\"bytes\":{d},\"raw_sha256\":\"{s}\",\"events\":{d},\"tool_calls\":{d},\"model\":{f},\"head_ms\":{d:.2},\"total_ms\":{d:.2},\"holds\":{d},\"held_ms\":{d:.2},\"malformed\":{d}", .{
            id,                         resp.status,                    ctx.bytes + resp.raw.len, std.fmt.bytesToHex(d, .lower), ctx.watcher.events,
            ctx.watcher.done.items.len, j(ctx.watcher.model orelse ""), t_head - t_sent,          t_end - t_sent,                ctx.holds,
            ctx.held_ms_total,          ctx.watcher.malformed,
        });
        if (!keep) return;
    }
}

fn env(name: []const u8) ?[]const u8 {
    return posix.getenv(name);
}

pub fn main() !void {
    cfg = .{
        .port = try std.fmt.parseInt(u16, env("LISTEN_PORT") orelse "443", 10),
        .upstream_ip = env("UPSTREAM_IP") orelse "127.0.0.1",
        .upstream_port = try std.fmt.parseInt(u16, env("UPSTREAM_PORT") orelse "8443", 10),
        .deny_substr = env("DENY_SUBSTR"),
        .probe_ctl = env("PROBE_CTL"),
        .probe_ack = env("PROBE_ACK"),
        .record = env("RECORD") orelse "record.jsonl",
        .hold = !std.mem.eql(u8, env("HOLD") orelse "1", "0"),
    };
    record_fd = try posix.open(cfg.record, .{ .ACCMODE = .WRONLY, .CREAT = true, .APPEND = true }, 0o644);
    if (env("SILO_CONFIG")) |p| state = try loadState(p);
    if (cfg.probe_ctl) |p| probe_ctl_fd = try posix.open(p, .{ .ACCMODE = .RDWR }, 0);
    if (cfg.probe_ack) |p| probe_ack_fd = try posix.open(p, .{ .ACCMODE = .RDWR, .NONBLOCK = true }, 0);

    server_ctx = c.SSL_CTX_new(c.TLS_server_method());
    _ = c.SSL_CTX_ctrl(server_ctx, c.SSL_CTRL_SET_MIN_PROTO_VERSION, c.TLS1_2_VERSION, null);
    const cert = try gpa.dupeZ(u8, env("CERT") orelse "cert.pem");
    const key = try gpa.dupeZ(u8, env("KEY") orelse "key.pem");
    if (c.SSL_CTX_use_certificate_chain_file(server_ctx, cert.ptr) != 1) return sslErr("cert");
    if (c.SSL_CTX_use_PrivateKey_file(server_ctx, key.ptr, c.SSL_FILETYPE_PEM) != 1) return sslErr("key");
    c.SSL_CTX_set_alpn_select_cb(server_ctx, alpnSelect, null);
    _ = c.SSL_CTX_set_options(server_ctx, @as(u64, 1) << 7);

    client_ctx = c.SSL_CTX_new(c.TLS_client_method());
    _ = c.SSL_CTX_ctrl(client_ctx, c.SSL_CTRL_SET_MIN_PROTO_VERSION, c.TLS1_2_VERSION, null);
    // A body framed by the end of the connection ends when the host closes it. The real providers
    // frame their bodies, so this only matters for a fake host; see the notes.
    _ = c.SSL_CTX_set_options(client_ctx, @as(u64, 1) << 7);
    const ca = try gpa.dupeZ(u8, env("UPSTREAM_CA") orelse "upstream-ca.pem");
    if (c.SSL_CTX_load_verify_locations(client_ctx, ca.ptr, null) != 1) return sslErr("upstream ca");
    c.SSL_CTX_set_verify(client_ctx, c.SSL_VERIFY_PEER, null);

    const addr = try std.net.Address.parseIp4(env("LISTEN_IP") orelse "0.0.0.0", cfg.port);
    var server = try addr.listen(.{ .reuse_address = true });
    record("\"kind\":\"listening\",\"port\":{d},\"hold\":{},\"policy\":\"{s}\"", .{ cfg.port, cfg.hold, if (state) |st| @tagName(st.book.policy) else "none" });
    while (true) {
        const conn = try server.accept();
        const t = try std.Thread.spawn(.{}, handle, .{conn.stream.handle});
        t.detach();
    }
}
