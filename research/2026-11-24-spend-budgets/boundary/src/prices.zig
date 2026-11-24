//! Silo's own price table: one row per provider, model, tier and date of effect, in dollars per
//! million tokens. Read from a JSON file, and read again whenever the file changes, so a price
//! that changes on a date, or a new model, needs no restart.
const std = @import("std");
const usage = @import("usage.zig");
const Allocator = std.mem.Allocator;

pub const Row = struct {
    provider: []const u8,
    /// The model id. A trailing `*` matches any id that starts with what comes before it, for
    /// dated ids such as `claude-opus-5-5-20261101`.
    model: []const u8,
    /// "standard" unless the provider says another ran: "fast" (Anthropic `usage.speed`),
    /// "priority", "flex", "ultrafast", "batch".
    tier: []const u8 = "standard",
    /// The first day this row applies, YYYY-MM-DD. The latest row not after today wins.
    from: []const u8 = "2000-01-01",
    input: f64,
    output: f64,
    cache_read: f64 = 0,
    cache_write_short: f64 = 0,
    cache_write_long: f64 = 0,
    /// Above this many input tokens in one call (cached ones included) the long rates apply.
    long_above: ?i64 = null,
    long_input_mult: f64 = 1,
    long_output_mult: f64 = 1,
    web_search_per_1k: f64 = 0,
};

const File = struct { rows: []Row };

pub const Table = struct {
    gpa: Allocator,
    path: []const u8,
    mtime: i128 = -1,
    parsed: ?std.json.Parsed(File) = null,
    mu: std.Thread.Mutex = .{},
    reloads: u32 = 0,

    pub fn deinit(self: *Table) void {
        if (self.parsed) |*p| p.deinit();
    }

    /// Reads the file again if it changed. A file that does not parse keeps the last good table.
    pub fn refresh(self: *Table) void {
        const st = std.fs.cwd().statFile(self.path) catch return;
        if (st.mtime == self.mtime) return;
        const bytes = std.fs.cwd().readFileAlloc(self.gpa, self.path, 16 << 20) catch return;
        defer self.gpa.free(bytes);
        const p = std.json.parseFromSlice(File, self.gpa, bytes, .{ .ignore_unknown_fields = true, .allocate = .alloc_always }) catch |e| {
            std.log.warn("price table {s} does not parse: {s}; keeping the last one", .{ self.path, @errorName(e) });
            self.mtime = st.mtime;
            return;
        };
        if (self.parsed) |*old| old.deinit();
        self.parsed = p;
        self.mtime = st.mtime;
        self.reloads += 1;
    }

    /// The row for a model on a day, or null: a model Silo has no price for.
    pub fn find(self: *Table, provider: []const u8, model: []const u8, tier: []const u8, day: []const u8) ?Row {
        self.mu.lock();
        defer self.mu.unlock();
        self.refresh();
        const p = self.parsed orelse return null;
        return pick(p.value.rows, provider, model, tier, day);
    }
};

pub fn pick(rows: []const Row, provider: []const u8, model: []const u8, tier: []const u8, day: []const u8) ?Row {
    var best: ?Row = null;
    for (rows) |r| {
        if (!std.mem.eql(u8, r.provider, provider)) continue;
        if (!modelMatches(r.model, model)) continue;
        if (!std.mem.eql(u8, r.tier, tier)) continue;
        if (std.mem.order(u8, r.from, day) == .gt) continue;
        if (best) |b| {
            // An exact id beats a prefix; then the latest date of effect.
            const b_exact = !std.mem.endsWith(u8, b.model, "*");
            const r_exact = !std.mem.endsWith(u8, r.model, "*");
            if (b_exact and !r_exact) continue;
            if (r_exact == b_exact and std.mem.order(u8, r.from, b.from) != .gt) continue;
        }
        best = r;
    }
    return best;
}

fn modelMatches(pattern: []const u8, model: []const u8) bool {
    if (std.mem.endsWith(u8, pattern, "*")) return std.mem.startsWith(u8, model, pattern[0 .. pattern.len - 1]);
    return std.mem.eql(u8, pattern, model);
}

/// What `c` costs at row `r`, in dollars.
pub fn cost(r: Row, c: usage.Counts) f64 {
    const long = if (r.long_above) |t| c.totalInput() > t else false;
    const im: f64 = if (long) r.long_input_mult else 1;
    const om: f64 = if (long) r.long_output_mult else 1;
    const m = 1e-6;
    return im * m * (f(c.input) * r.input + f(c.cache_read) * r.cache_read + f(c.cache_write_short) * r.cache_write_short + f(c.cache_write_long) * r.cache_write_long) +
        om * m * f(c.output) * r.output + f(c.web_searches) * r.web_search_per_1k / 1000;
}

/// The highest price per input token this row can charge: every token written to the dearest
/// cache, at the long-context rate if the call is long.
pub fn dearestInput(r: Row, tokens: i64) f64 {
    const long = if (r.long_above) |t| tokens > t else false;
    const im: f64 = if (long) r.long_input_mult else 1;
    return im * @max(r.input, @max(r.cache_write_short, r.cache_write_long));
}

/// The output price per token, at the long-context rate if `input_tokens` makes the call long.
pub fn outputRate(r: Row, input_tokens: i64) f64 {
    const long = if (r.long_above) |t| input_tokens > t else false;
    return (if (long) r.long_output_mult else 1) * r.output;
}

fn f(x: i64) f64 {
    return @floatFromInt(x);
}

/// Today as YYYY-MM-DD, or the day the environment variable SILO_TODAY names (for tests of a
/// price that changes on a date).
pub fn today(buf: *[10]u8) []const u8 {
    if (std.posix.getenv("SILO_TODAY")) |d| if (d.len == 10) {
        @memcpy(buf, d);
        return buf;
    };
    const secs: u64 = @intCast(std.time.timestamp());
    const ed = std.time.epoch.EpochSeconds{ .secs = secs };
    const yd = ed.getEpochDay().calculateYearDay();
    const md = yd.calculateMonthDay();
    _ = std.fmt.bufPrint(buf, "{d:0>4}-{d:0>2}-{d:0>2}", .{ yd.year, md.month.numeric(), md.day_index + 1 }) catch unreachable;
    return buf;
}

test "the latest row not after the day, exact before prefix" {
    const rows = [_]Row{
        .{ .provider = "google", .model = "gemini-3.8-flash", .from = "2026-01-01", .input = 0.75, .output = 3.75 },
        .{ .provider = "google", .model = "gemini-3.8-flash", .from = "2027-01-01", .input = 1.5, .output = 7.5 },
        .{ .provider = "anthropic", .model = "claude-opus-5-5*", .input = 9, .output = 9 },
        .{ .provider = "anthropic", .model = "claude-opus-5-5", .input = 4, .output = 20 },
        .{ .provider = "anthropic", .model = "claude-opus-5-5", .tier = "fast", .input = 8, .output = 40 },
    };
    try std.testing.expectEqual(@as(f64, 0.75), pick(&rows, "google", "gemini-3.8-flash", "standard", "2026-12-31").?.input);
    try std.testing.expectEqual(@as(f64, 1.5), pick(&rows, "google", "gemini-3.8-flash", "standard", "2027-01-01").?.input);
    try std.testing.expectEqual(@as(f64, 4), pick(&rows, "anthropic", "claude-opus-5-5", "standard", "2026-11-24").?.input);
    try std.testing.expectEqual(@as(f64, 9), pick(&rows, "anthropic", "claude-opus-5-5-20261101", "standard", "2026-11-24").?.input);
    try std.testing.expectEqual(@as(f64, 8), pick(&rows, "anthropic", "claude-opus-5-5", "fast", "2026-11-24").?.input);
    try std.testing.expect(pick(&rows, "anthropic", "claude-unknown", "standard", "2026-11-24") == null);
}

test "cost with cache and long context" {
    const r: Row = .{ .provider = "openai", .model = "gpt-6.2-sol", .input = 2, .output = 10, .cache_read = 0.1, .cache_write_short = 2.5, .cache_write_long = 5, .long_above = 272000, .long_input_mult = 2, .long_output_mult = 1.5 };
    const short: usage.Counts = .{ .input = 1000, .cache_read = 100000, .output = 1000 };
    try std.testing.expectApproxEqAbs(@as(f64, 0.002 + 0.01 + 0.01), cost(r, short), 1e-9);
    const long: usage.Counts = .{ .input = 300000, .output = 1000 };
    try std.testing.expectApproxEqAbs(@as(f64, 1.2 + 0.015), cost(r, long), 1e-9);
    try std.testing.expectEqual(@as(f64, 10), dearestInput(r, 300000));
}
