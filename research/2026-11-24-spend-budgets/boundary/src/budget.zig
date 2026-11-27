//! Budgets: what each computer and each person has spent on models, and whether one more model
//! call may leave. A call is admitted against every budget that covers it, counted while it
//! streams, and settled on the provider's final count. Pure: the boundary passes the time in.
const std = @import("std");
const Allocator = std.mem.Allocator;
pub const journal = @import("journal.zig");

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
    /// Dollars each conversation on this computer may spend. The boundary keeps one ledger per
    /// conversation it recognises, with this as its total.
    conversation_total: ?f64 = null,
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
    /// When set, every call that leaves and every settlement is on disk before the call goes on,
    /// and a ledger added starts at what the journal says it spent (journal.zig).
    journal: ?*journal.Journal = null,

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
        self.mu.lock();
        defer self.mu.unlock();
        try self.ledgers.append(self.gpa, .{ .name = name, .limits = limits });
        try self.carryOver(&self.ledgers.items[self.ledgers.items.len - 1]);
        return self.ledgers.items.len - 1;
    }

    /// What the journal kept for this ledger, from before the boundary last started.
    fn carryOver(self: *Book, l: *Ledger) !void {
        const j = self.journal orelse return;
        const c = j.carried.get(l.name) orelse return;
        l.settled += c.settled;
        l.calls += c.calls;
        for (c.history.items) |e| try l.history.append(self.gpa, .{ .t = e.t, .cost = e.cost });
        if (l.limits.total) |total| if (l.settled >= total) {
            l.reached_at = if (c.history.items.len > 0) c.history.items[c.history.items.len - 1].t else 0;
        };
    }

    /// A new ledger that starts at what another has counted now: a fork's copy of a conversation
    /// that carries its spend.
    pub fn addFrom(self: *Book, name: []const u8, limits: Limits, from: usize) !usize {
        self.mu.lock();
        defer self.mu.unlock();
        const begin = self.ledgers.items[from].counted();
        try self.ledgers.append(self.gpa, .{ .name = name, .limits = limits, .settled = begin });
        try self.carryOver(&self.ledgers.items[self.ledgers.items.len - 1]);
        return self.ledgers.items.len - 1;
    }

    /// What a ledger has settled, and counts now with its calls under way.
    pub fn spent(self: *Book, i: usize) struct { settled: f64, counted: f64, total: ?f64, name: []const u8 } {
        self.mu.lock();
        defer self.mu.unlock();
        const l = &self.ledgers.items[i];
        return .{ .settled = l.settled, .counted = l.counted(), .total = l.limits.total, .name = l.name };
    }

    /// Whether work that runs out of the boundary's sight may start: its whole bound must fit
    /// under every total, whatever the policy, since nothing can stop it once it has left.
    pub fn admitWhole(self: *Book, which: []const usize, bound: f64) ?Refusal {
        self.mu.lock();
        defer self.mu.unlock();
        for (which) |i| {
            const l = &self.ledgers.items[i];
            const total = l.limits.total orelse continue;
            const now_held = l.held();
            if (now_held + bound > total) {
                l.refused += 1;
                return .{ .ledger = l.name, .why = if (l.counted() >= total) .total else .input, .limit = total, .counted = now_held, .needed = bound };
            }
        }
        return null;
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
        if (self.journal) |j| {
            // On disk before the call leaves: a boundary stopped while it runs counts it at its
            // reserve when it starts again. A journal that cannot be written stops the call.
            var names: [16][]const u8 = undefined;
            const n = @min(which.len, names.len);
            for (which[0..n], 0..) |i, k| names[k] = self.ledgers.items[i].name;
            try j.started(id, reserve, @as(f64, @floatFromInt(std.time.milliTimestamp())) / 1000, names[0..n]);
        }
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
        if (self.journal) |j| try j.settled(id, cost, now);
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

test "work out of sight is admitted only if its whole bound fits, and holds it" {
    var b: Book = .{ .gpa = std.testing.allocator };
    defer b.deinit();
    const c = try b.add("main", .{ .total = 1.0 });
    const w = [_]usize{c};
    try std.testing.expect(b.admitWhole(&w, 0.6) == null);
    try b.start(&w, 1, 0.6);
    b.progress(&w, 1, 0.6);
    try std.testing.expect(b.admitWhole(&w, 0.5) != null);
    _ = try b.settle(&w, 1, 0.2, false, 1);
    try std.testing.expect(b.admitWhole(&w, 0.5) == null);
}

test "a carried ledger starts at what the other one counted" {
    var b: Book = .{ .gpa = std.testing.allocator };
    defer b.deinit();
    const a = try b.add("conv:1", .{ .total = 1.0 });
    _ = try b.settle(&[_]usize{a}, 1, 0.7, false, 0);
    const f = try b.addFrom("conv:2", .{ .total = 1.0 }, a);
    try std.testing.expectApproxEqAbs(@as(f64, 0.7), b.spent(f).settled, 1e-9);
}
