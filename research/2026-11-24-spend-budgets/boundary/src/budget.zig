//! Budgets: what each computer and each person has spent on models, and whether one more model
//! call may leave. A call is admitted against every budget that covers it, counted while it
//! streams, and settled on the provider's final count. Pure: the boundary passes the time in.
const std = @import("std");
const Allocator = std.mem.Allocator;

pub const Policy = enum {
    /// The plan today: a call leaves while what is counted is under the limit. Calls under way
    /// finish, even past it.
    finish,
    /// The same, and the call's own input, priced at the dearest rate it can incur, must fit in
    /// what remains. #351 proposed it.
    admit_input,
    /// LiteLLM's default: each call holds its dearest input plus `max_tokens` of output while it
    /// runs, and leaves only if that hold fits.
    reserve_max,
    /// The same hold, with the output the model typically writes instead of `max_tokens` (#351's
    /// `reserve_input`): the boundary passes a smaller reserve, the rule is the same.
    reserve_typical,
};

pub const Limits = struct {
    /// Dollars, at API prices.
    total: ?f64 = null,
    /// Dollars in any window of `rate_window_s` seconds (an hour unless set).
    rate: ?f64 = null,
    rate_window_s: f64 = 3600,
    /// The share of `total` at which the one alert goes out.
    alert_share: f64 = 0.8,
    /// The highest share of the provider's 5-hour or weekly window this computer may bring the
    /// account to, from 0 to 1, as the provider's headers report it for the whole account.
    plan_5h_max: ?f64 = null,
    plan_7d_max: ?f64 = null,
};

const Spent = struct { t: f64, cost: f64 };
const Inflight = struct { counted: f64, reserved: f64 };

pub const Ledger = struct {
    name: []const u8,
    limits: Limits,
    settled: f64 = 0,
    estimated: f64 = 0,
    calls: u32 = 0,
    refused: u32 = 0,
    history: std.ArrayList(Spent) = .empty,
    inflight: std.AutoHashMapUnmanaged(u64, Inflight) = .empty,
    alerted_at: ?f64 = null,
    reached_at: ?f64 = null,
    /// What this computer moved the account's plan windows by, summed over its own calls.
    plan_5h_moved: f64 = 0,
    plan_7d_moved: f64 = 0,

    fn counted(self: *const Ledger) f64 {
        var s = self.settled;
        var it = self.inflight.valueIterator();
        while (it.next()) |v| s += v.counted;
        return s;
    }
    fn held(self: *const Ledger) f64 {
        var s = self.settled;
        var it = self.inflight.valueIterator();
        while (it.next()) |v| s += @max(v.counted, v.reserved);
        return s;
    }
    fn inWindow(self: *const Ledger, now: f64) f64 {
        var s: f64 = 0;
        for (self.history.items) |e| {
            if (now - e.t < self.limits.rate_window_s) s += e.cost;
        }
        var it = self.inflight.valueIterator();
        while (it.next()) |v| s += v.counted;
        return s;
    }
};

pub const Refusal = struct {
    ledger: []const u8,
    why: enum { total, input, held, rate, plan_5h, plan_7d },
    limit: f64,
    counted: f64,
    /// For `input` and `held`: what this call would have needed.
    needed: f64 = 0,
    /// For a rate: when spend in the window falls under the limit again, if nothing else is spent.
    passes_again_at: ?f64 = null,
};

/// The latest reading of one account's plan windows, from the provider's reply headers.
pub const PlanReading = struct {
    five_hour: ?f64 = null,
    seven_day: ?f64 = null,
    t: f64 = 0,
};

pub const Book = struct {
    gpa: Allocator,
    policy: Policy = .finish,
    ledgers: std.ArrayList(Ledger) = .empty,
    plans: std.StringHashMapUnmanaged(PlanReading) = .empty,
    mu: std.Thread.Mutex = .{},

    pub fn deinit(self: *Book) void {
        for (self.ledgers.items) |*l| {
            l.history.deinit(self.gpa);
            l.inflight.deinit(self.gpa);
        }
        self.ledgers.deinit(self.gpa);
        var it = self.plans.keyIterator();
        while (it.next()) |k| self.gpa.free(k.*);
        self.plans.deinit(self.gpa);
    }

    pub fn add(self: *Book, name: []const u8, limits: Limits) !usize {
        try self.ledgers.append(self.gpa, .{ .name = name, .limits = limits });
        return self.ledgers.items.len - 1;
    }

    pub fn find(self: *Book, name: []const u8) ?usize {
        for (self.ledgers.items, 0..) |l, i| if (std.mem.eql(u8, l.name, name)) return i;
        return null;
    }

    /// Whether a call may leave, against every ledger in `which`. `input_cost` is the call's
    /// input priced at the dearest rate it can incur; `reserve` adds `max_tokens` of output.
    /// `account` names the plan whose windows the call draws on, when it is a subscription.
    pub fn admit(self: *Book, which: []const usize, input_cost: f64, reserve: f64, account: ?[]const u8, now: f64) ?Refusal {
        self.mu.lock();
        defer self.mu.unlock();
        for (which) |i| {
            const l = &self.ledgers.items[i];
            if (l.limits.total) |total| {
                const refuse = switch (self.policy) {
                    .finish => l.counted() >= total,
                    .admit_input => l.counted() + input_cost > total,
                    .reserve_max, .reserve_typical => l.held() + reserve > total,
                };
                if (refuse) {
                    l.refused += 1;
                    // Reached, or this call alone does not fit in what is left, or what the calls
                    // under way hold leaves no room for it.
                    const why: @TypeOf(@as(Refusal, undefined).why) = if (l.counted() >= total) .total else if (self.policy == .admit_input) .input else .held;
                    if (why == .total and l.reached_at == null) l.reached_at = now;
                    return .{ .ledger = l.name, .why = why, .limit = total, .counted = l.counted(), .needed = if (why == .input) input_cost else reserve };
                }
            }
            if (l.limits.rate) |rate| {
                const in_window = l.inWindow(now);
                const extra: f64 = if (self.policy == .finish) 0 else input_cost;
                if (in_window + extra >= rate) {
                    l.refused += 1;
                    return .{ .ledger = l.name, .why = .rate, .limit = rate, .counted = in_window, .passes_again_at = passesAgain(l, now, rate - extra) };
                }
            }
            if (account) |a| if (self.plans.get(a)) |r| {
                if (l.limits.plan_5h_max) |m| if (r.five_hour) |u| if (u >= m) {
                    l.refused += 1;
                    return .{ .ledger = l.name, .why = .plan_5h, .limit = m, .counted = u };
                };
                if (l.limits.plan_7d_max) |m| if (r.seven_day) |u| if (u >= m) {
                    l.refused += 1;
                    return .{ .ledger = l.name, .why = .plan_7d, .limit = m, .counted = u };
                };
            };
        }
        return null;
    }

    /// The call left: it counts in every ledger from now on.
    pub fn start(self: *Book, which: []const usize, id: u64, reserve: f64) !void {
        self.mu.lock();
        defer self.mu.unlock();
        for (which) |i| try self.ledgers.items[i].inflight.put(self.gpa, id, .{ .counted = 0, .reserved = reserve });
    }

    /// What the call has cost so far, as far as the boundary can see: its input once known, and
    /// the visible output.
    pub fn progress(self: *Book, which: []const usize, id: u64, counted: f64) void {
        self.mu.lock();
        defer self.mu.unlock();
        for (which) |i| if (self.ledgers.items[i].inflight.getPtr(id)) |v| {
            v.counted = counted;
        };
    }

    pub const Settled = struct { alert: ?[]const u8 = null, reached: ?[]const u8 = null };

    /// The call ended. `estimated` says the provider gave no final count.
    pub fn settle(self: *Book, which: []const usize, id: u64, cost: f64, estimated: bool, now: f64) !Settled {
        self.mu.lock();
        defer self.mu.unlock();
        var out: Settled = .{};
        for (which) |i| {
            const l = &self.ledgers.items[i];
            _ = l.inflight.remove(id);
            l.settled += cost;
            if (estimated) l.estimated += cost;
            l.calls += 1;
            try l.history.append(self.gpa, .{ .t = now, .cost = cost });
            if (l.limits.total) |total| {
                if (l.alerted_at == null and l.settled >= l.limits.alert_share * total) {
                    l.alerted_at = now;
                    out.alert = l.name;
                }
                if (l.reached_at == null and l.settled >= total) {
                    l.reached_at = now;
                    out.reached = l.name;
                }
            }
        }
        return out;
    }

    /// A reply carried the account's plan windows. The change since the last reading is put on
    /// this call's ledgers: a share of a window the account's other users also draw on.
    pub fn planReading(self: *Book, which: []const usize, account: []const u8, five_hour: ?f64, seven_day: ?f64, now: f64) !void {
        self.mu.lock();
        defer self.mu.unlock();
        const gop = try self.plans.getOrPut(self.gpa, account);
        if (!gop.found_existing) {
            gop.key_ptr.* = try self.gpa.dupe(u8, account);
            gop.value_ptr.* = .{};
        }
        const prev = gop.value_ptr.*;
        for (which) |i| {
            const l = &self.ledgers.items[i];
            if (five_hour) |u| if (prev.five_hour) |p| if (u > p) {
                l.plan_5h_moved += u - p;
            };
            if (seven_day) |u| if (prev.seven_day) |p| if (u > p) {
                l.plan_7d_moved += u - p;
            };
        }
        gop.value_ptr.* = .{ .five_hour = five_hour orelse prev.five_hour, .seven_day = seven_day orelse prev.seven_day, .t = now };
    }
};

fn passesAgain(l: *const Ledger, now: f64, under: f64) ?f64 {
    // Walk the history from the oldest call in the window until the rest is under the limit.
    var s = l.inWindow(now);
    for (l.history.items) |e| {
        if (now - e.t >= l.limits.rate_window_s) continue;
        s -= e.cost;
        if (s < under) return e.t + l.limits.rate_window_s;
    }
    return null;
}

// ---- tests ----

test "finish lets calls under way end past the limit, and stops the next" {
    var b: Book = .{ .gpa = std.testing.allocator };
    defer b.deinit();
    const c = try b.add("main", .{ .total = 1.0 });
    const w = [_]usize{c};
    try std.testing.expect(b.admit(&w, 0.1, 0, null, 0) == null);
    try b.start(&w, 1, 0);
    try std.testing.expect(b.admit(&w, 0.1, 0, null, 0) == null);
    try b.start(&w, 2, 0);
    b.progress(&w, 1, 0.7);
    _ = try b.settle(&w, 1, 0.8, false, 1);
    const s = try b.settle(&w, 2, 0.5, false, 2);
    try std.testing.expectEqualStrings("main", s.reached.?);
    try std.testing.expect(b.admit(&w, 0.0, 0, null, 3) != null);
    try std.testing.expectApproxEqAbs(@as(f64, 1.3), b.ledgers.items[c].settled, 1e-9);
}

test "admit_input refuses a call whose input alone would pass the limit" {
    var b: Book = .{ .gpa = std.testing.allocator, .policy = .admit_input };
    defer b.deinit();
    const c = try b.add("main", .{ .total = 1.0 });
    const w = [_]usize{c};
    _ = try b.settle(&w, 1, 0.6, false, 0);
    try std.testing.expect(b.admit(&w, 0.3, 0, null, 1) == null);
    try std.testing.expect(b.admit(&w, 0.5, 0, null, 1) != null);
}

test "every budget that covers a call applies" {
    var b: Book = .{ .gpa = std.testing.allocator };
    defer b.deinit();
    const comp = try b.add("main", .{ .total = 10 });
    const person = try b.add("person:ana", .{ .total = 1 });
    const w = [_]usize{ comp, person };
    _ = try b.settle(&w, 1, 1.0, false, 0);
    const r = b.admit(&w, 0, 0, null, 1).?;
    try std.testing.expectEqualStrings("person:ana", r.ledger);
}

test "a rate passes again once the window has moved on" {
    var b: Book = .{ .gpa = std.testing.allocator };
    defer b.deinit();
    const c = try b.add("main", .{ .rate = 1, .rate_window_s = 60 });
    const w = [_]usize{c};
    _ = try b.settle(&w, 1, 0.6, false, 0);
    _ = try b.settle(&w, 2, 0.6, false, 10);
    const r = b.admit(&w, 0, 0, null, 20).?;
    try std.testing.expectEqual(@as(f64, 60), r.passes_again_at.?);
    try std.testing.expect(b.admit(&w, 0, 0, null, 61) == null);
}

test "a plan window refuses from the account's reading, and the computer's share is the deltas" {
    var b: Book = .{ .gpa = std.testing.allocator };
    defer b.deinit();
    const c = try b.add("main", .{ .plan_5h_max = 0.5 });
    const w = [_]usize{c};
    try b.planReading(&w, "acct", 0.30, 0.10, 0);
    try std.testing.expect(b.admit(&w, 0, 0, "acct", 1) == null);
    try b.planReading(&w, "acct", 0.42, 0.12, 2);
    try b.planReading(&w, "acct", 0.51, 0.13, 3);
    try std.testing.expect(b.admit(&w, 0, 0, "acct", 4).?.why == .plan_5h);
    try std.testing.expectApproxEqAbs(@as(f64, 0.21), b.ledgers.items[c].plan_5h_moved, 1e-9);
}
