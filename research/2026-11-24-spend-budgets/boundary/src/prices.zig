//! Silo's own price table: one row per provider, model, tier and date of effect, in dollars per
//! million tokens. Read from a JSON file, and read again whenever the file changes, so a price
//! that changes on a date, or a new model, needs no restart.
//!
//! With keys set, the file is a signed release (research/2026-11-25-model-prices): it is taken
//! only with a valid Ed25519 signature in `<file>.sig` by one of the keys, and only if its version
//! is above every version taken before, which `<file>.seen` remembers across restarts. A file that
//! fails either keeps the last good table. A local file of the user's own rows, unsigned, is read
//! beside it and wins over it.
const std = @import("std");
const journal = @import("journal.zig");
const usage = @import("usage.zig");
const Allocator = std.mem.Allocator;
const Ed25519 = std.crypto.sign.Ed25519;

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
    /// Gemini's explicit cache: dollars per million tokens per hour it is kept.
    cache_storage_per_hour: f64 = 0,
    /// Where the row was read, and the day it was last checked there. Not used to price.
    source: []const u8 = "",
    checked: []const u8 = "",
};

/// A factor on every price of the rows it matches, when the request asks for it: Anthropic's
/// US-only inference (`inference_geo: "us"`, 1.1x on Claude 4.6 and later), or a discount the user
/// writes in their own file.
pub const Multiplier = struct {
    provider: []const u8,
    /// Model patterns as in `Row.model`; empty matches every model of the provider.
    models: []const []const u8 = &.{},
    /// What the request must ask for: "inference_geo=us", or "" for always.
    when: []const u8 = "",
    factor: f64,
    from: []const u8 = "2000-01-01",
};

pub const File = struct {
    /// 1: #374's file, rows only. 2: a release, with what follows.
    format: u32 = 1,
    /// Above every earlier release's. Silo refuses a release whose version is not.
    version: u64 = 0,
    published: []const u8 = "",
    /// After this day the app says the table may be out of date. It is still used.
    check_by: []const u8 = "",
    rows: []Row = &.{},
    multipliers: []Multiplier = &.{},
};

pub const Key = struct {
    id: []const u8,
    /// The Ed25519 public key, 32 bytes in base64.
    public: []const u8,
};

/// What a reading of the files did, for the record.
pub const Event = enum { loaded, unchanged, bad_signature, no_signature, unknown_key, rolled_back, unparsable, local_loaded, local_unparsable };

pub const Table = struct {
    gpa: Allocator,
    path: []const u8,
    /// With keys, the file must be a signed release.
    keys: []const Key = &.{},
    /// The user's own rows and multipliers, unsigned, which win over the release.
    local_path: ?[]const u8 = null,
    stamp: u64 = 0,
    local_stamp: u64 = 0,
    parsed: ?std.json.Parsed(File) = null,
    local: ?std.json.Parsed(File) = null,
    mu: std.Thread.Mutex = .{},
    reloads: u32 = 0,
    /// Told of every reading that changed something or was refused.
    on_event: ?*const fn (Event, u64, []const u8) void = null,

    pub fn deinit(self: *Table) void {
        if (self.parsed) |*p| p.deinit();
        if (self.local) |*p| p.deinit();
    }

    fn tell(self: *Table, e: Event, v: u64, why: []const u8) void {
        if (self.on_event) |cb| cb(e, v, why) else if (e != .loaded and e != .unchanged and e != .local_loaded) std.log.warn("price table {s}: {s} {s}", .{ self.path, @tagName(e), why });
    }

    /// Reads the files again if they changed. A file that does not parse, or a release that is not
    /// signed or not newer, keeps the last good table.
    pub fn refresh(self: *Table) void {
        const st = stampOf(self.path, if (self.keys.len > 0) ".sig" else null);
        if (st != self.stamp) {
            self.stamp = st;
            self.load();
        }
        if (self.local_path) |lp| {
            const ls = stampOf(lp, null);
            if (ls != self.local_stamp) {
                self.local_stamp = ls;
                self.loadLocal(lp);
            }
        }
    }

    fn load(self: *Table) void {
        const bytes = std.fs.cwd().readFileAlloc(self.gpa, self.path, 16 << 20) catch return;
        defer self.gpa.free(bytes);
        if (self.keys.len > 0) {
            const why = self.verify(bytes);
            if (why) |e| return self.tell(e, 0, "kept the last good table");
        }
        const p = std.json.parseFromSlice(File, self.gpa, bytes, .{ .ignore_unknown_fields = true, .allocate = .alloc_always }) catch |e| {
            return self.tell(.unparsable, 0, @errorName(e));
        };
        if (self.keys.len > 0) {
            var digest: [32]u8 = undefined;
            std.crypto.hash.sha2.Sha256.hash(bytes, &digest, .{});
            const seen = self.readSeen();
            const v = p.value.version;
            // The same version is taken again only with the same bytes; a lower one never.
            if (seen) |s| if (v < s.version or (v == s.version and !std.mem.eql(u8, &digest, &s.digest))) {
                var pp = p;
                pp.deinit();
                var buf: [64]u8 = undefined;
                return self.tell(.rolled_back, v, std.fmt.bufPrint(&buf, "{d} is not above {d}", .{ v, s.version }) catch "");
            };
            self.writeSeen(v, digest);
        }
        if (self.parsed) |*old| old.deinit();
        self.parsed = p;
        self.reloads += 1;
        self.tell(.loaded, p.value.version, "");
    }

    fn loadLocal(self: *Table, lp: []const u8) void {
        const bytes = std.fs.cwd().readFileAlloc(self.gpa, lp, 1 << 20) catch {
            if (self.local) |*old| old.deinit();
            self.local = null;
            return;
        };
        defer self.gpa.free(bytes);
        const p = std.json.parseFromSlice(File, self.gpa, bytes, .{ .ignore_unknown_fields = true, .allocate = .alloc_always }) catch |e| {
            return self.tell(.local_unparsable, 0, @errorName(e));
        };
        if (self.local) |*old| old.deinit();
        self.local = p;
        self.tell(.local_loaded, 0, "");
    }

    /// Null when one of the keys signed exactly these bytes.
    fn verify(self: *Table, bytes: []const u8) ?Event {
        var pbuf: [512]u8 = undefined;
        const sp = std.fmt.bufPrint(&pbuf, "{s}.sig", .{self.path}) catch return .no_signature;
        var sbuf: [512]u8 = undefined;
        const line = std.fs.cwd().readFile(sp, &sbuf) catch return .no_signature;
        return verifyLine(self.keys, bytes, line);
    }

    const Seen = struct { version: u64, digest: [32]u8 };

    fn readSeen(self: *Table) ?Seen {
        var pbuf: [512]u8 = undefined;
        const sp = std.fmt.bufPrint(&pbuf, "{s}.seen", .{self.path}) catch return null;
        var buf: [128]u8 = undefined;
        const s = std.mem.trim(u8, std.fs.cwd().readFile(sp, &buf) catch return null, " \n");
        const sp_i = std.mem.indexOfScalar(u8, s, ' ') orelse return null;
        var out: Seen = .{ .version = std.fmt.parseInt(u64, s[0..sp_i], 10) catch return null, .digest = undefined };
        _ = std.fmt.hexToBytes(&out.digest, s[sp_i + 1 ..]) catch return null;
        return out;
    }

    fn writeSeen(self: *Table, v: u64, digest: [32]u8) void {
        var pbuf: [512]u8 = undefined;
        const sp = std.fmt.bufPrint(&pbuf, "{s}.seen", .{self.path}) catch return;
        var buf: [128]u8 = undefined;
        const line = std.fmt.bufPrint(&buf, "{d} {x}\n", .{ v, digest }) catch return;
        // Written aside, flushed, then renamed over the old one and the folder flushed: a reader,
        // or a start after a power cut, finds the old line or the new one, never an empty file
        // that would let an older release in (research/2026-11-25-durability-on-a-mac).
        var tbuf: [520]u8 = undefined;
        const tp = std.fmt.bufPrint(&tbuf, "{s}.tmp", .{sp}) catch return;
        const tf = std.fs.cwd().createFile(tp, .{}) catch return;
        tf.writeAll(line) catch {
            tf.close();
            return;
        };
        journal.durable(tf.handle, .full) catch {};
        tf.close();
        std.fs.cwd().rename(tp, sp) catch return;
        var dir = std.fs.cwd().openDir(std.fs.path.dirname(sp) orelse ".", .{ .iterate = true }) catch return;
        defer dir.close();
        journal.durable(dir.fd, .full) catch {};
    }

    /// The version of the release in use, 0 for an unsigned file.
    pub fn version(self: *Table) u64 {
        const p = self.parsed orelse return 0;
        return p.value.version;
    }

    /// The row for a model on a day, or null: a model Silo has no price for. The user's own rows
    /// come first.
    pub fn find(self: *Table, provider: []const u8, model: []const u8, tier: []const u8, day: []const u8) ?Row {
        self.mu.lock();
        defer self.mu.unlock();
        self.refresh();
        if (self.local) |l| if (pick(l.value.rows, provider, model, tier, day)) |r| return r;
        const p = self.parsed orelse return null;
        return pick(p.value.rows, provider, model, tier, day);
    }

    /// As `find`, with the multipliers that apply to what the request asked (`asked`, such as
    /// "inference_geo=us"), the release's and then the user's, applied to every price of the row.
    pub fn findFor(self: *Table, provider: []const u8, model: []const u8, tier: []const u8, asked: []const []const u8, day: []const u8) ?Row {
        var r = self.find(provider, model, tier, day) orelse return null;
        self.mu.lock();
        defer self.mu.unlock();
        if (self.parsed) |p| r = applyMultipliers(r, p.value.multipliers, provider, model, asked, day);
        if (self.local) |l| r = applyMultipliers(r, l.value.multipliers, provider, model, asked, day);
        return r;
    }

    /// For a model with no row: every price at the dearest any row of that provider and tier
    /// charges on the day, times `margin`. A bound while the provider's new models cost no more
    /// than `margin` times its dearest known one: of 100 new models since 2025, 7 cost more than
    /// the dearest their provider sold, the most by 1.6x (research/2026-11-25-model-prices).
    pub fn bound(self: *Table, provider: []const u8, tier: []const u8, day: []const u8, margin: f64) ?Row {
        self.mu.lock();
        defer self.mu.unlock();
        self.refresh();
        const p = self.parsed orelse return null;
        var b: ?Row = null;
        for (p.value.rows) |r| {
            if (!std.mem.eql(u8, r.provider, provider) or !std.mem.eql(u8, r.tier, tier)) continue;
            if (std.mem.order(u8, r.from, day) == .gt) continue;
            if (b == null) {
                b = r;
                b.?.model = "(no price: the dearest known)";
                b.?.long_above = null;
                b.?.source = "";
                continue;
            }
            var x = &b.?;
            x.input = @max(x.input, r.input * r.long_input_mult);
            x.output = @max(x.output, r.output * r.long_output_mult);
            x.cache_read = @max(x.cache_read, r.cache_read * r.long_input_mult);
            x.cache_write_short = @max(x.cache_write_short, r.cache_write_short * r.long_input_mult);
            x.cache_write_long = @max(x.cache_write_long, r.cache_write_long * r.long_input_mult);
            x.web_search_per_1k = @max(x.web_search_per_1k, r.web_search_per_1k);
            x.cache_storage_per_hour = @max(x.cache_storage_per_hour, r.cache_storage_per_hour);
        }
        if (b) |*x| {
            x.input *= margin;
            x.output *= margin;
            x.cache_read *= margin;
            x.cache_write_short *= margin;
            x.cache_write_long *= margin;
            x.long_input_mult = 1;
            x.long_output_mult = 1;
        }
        return b;
    }
};

/// Checks a signature line, `<key id> <base64 signature>`, against the keys.
pub fn verifyLine(keys: []const Key, bytes: []const u8, line_raw: []const u8) ?Event {
    const line = std.mem.trim(u8, line_raw, " \r\n");
    const sp = std.mem.indexOfScalar(u8, line, ' ') orelse return .no_signature;
    const id = line[0..sp];
    const sig_b64 = std.mem.trim(u8, line[sp + 1 ..], " ");
    for (keys) |k| {
        if (!std.mem.eql(u8, k.id, id)) continue;
        var pk_bytes: [32]u8 = undefined;
        std.base64.standard.Decoder.decode(&pk_bytes, k.public) catch return .unknown_key;
        var sig_bytes: [64]u8 = undefined;
        std.base64.standard.Decoder.decode(&sig_bytes, sig_b64) catch return .bad_signature;
        const pk = Ed25519.PublicKey.fromBytes(pk_bytes) catch return .unknown_key;
        const sig = Ed25519.Signature.fromBytes(sig_bytes);
        sig.verify(bytes, pk) catch return .bad_signature;
        return null;
    }
    return .unknown_key;
}

/// What changes when either file, or the signature beside it, changes: their sizes and times.
fn stampOf(path: []const u8, sig_suffix: ?[]const u8) u64 {
    var h = std.hash.Wyhash.init(0);
    if (std.fs.cwd().statFile(path)) |st| {
        h.update(std.mem.asBytes(&st.mtime));
        h.update(std.mem.asBytes(&st.size));
    } else |_| h.update("none");
    if (sig_suffix) |sfx| {
        var buf: [512]u8 = undefined;
        if (std.fmt.bufPrint(&buf, "{s}{s}", .{ path, sfx })) |sp| {
            if (std.fs.cwd().statFile(sp)) |st| {
                h.update(std.mem.asBytes(&st.mtime));
                h.update(std.mem.asBytes(&st.size));
            } else |_| h.update("none");
        } else |_| {}
    }
    return h.final();
}

fn applyMultipliers(r0: Row, ms: []const Multiplier, provider: []const u8, model: []const u8, asked: []const []const u8, day: []const u8) Row {
    var r = r0;
    for (ms) |m| {
        if (!std.mem.eql(u8, m.provider, provider)) continue;
        if (std.mem.order(u8, m.from, day) == .gt) continue;
        if (m.models.len > 0) {
            var any = false;
            for (m.models) |pat| any = any or modelMatches(pat, model);
            if (!any) continue;
        }
        if (m.when.len > 0) {
            var has = false;
            for (asked) |a| has = has or std.mem.eql(u8, a, m.when);
            if (!has) continue;
        }
        r.input *= m.factor;
        r.output *= m.factor;
        r.cache_read *= m.factor;
        r.cache_write_short *= m.factor;
        r.cache_write_long *= m.factor;
        r.cache_storage_per_hour *= m.factor;
    }
    return r;
}

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

// ---- signed releases (research/2026-11-25-model-prices) ----

const TestRelease = struct {
    dir: std.testing.TmpDir,
    root: []u8,
    path: []u8,
    kp: Ed25519.KeyPair,
    pub_b64: [44]u8,

    fn init() !TestRelease {
        var t: TestRelease = undefined;
        t.dir = std.testing.tmpDir(.{});
        t.root = try t.dir.dir.realpathAlloc(std.testing.allocator, ".");
        t.path = try std.fmt.allocPrint(std.testing.allocator, "{s}/prices.json", .{t.root});
        t.kp = try Ed25519.KeyPair.generateDeterministic([_]u8{7} ** 32);
        _ = std.base64.standard.Encoder.encode(&t.pub_b64, &t.kp.public_key.toBytes());
        return t;
    }
    fn deinit(t: *TestRelease) void {
        std.testing.allocator.free(t.root);
        std.testing.allocator.free(t.path);
        t.dir.cleanup();
    }
    /// Writes a release and its signature by `kp`, as the release tool does.
    fn publish(t: *TestRelease, kp: Ed25519.KeyPair, key_id: []const u8, body: []const u8) !void {
        try std.fs.cwd().writeFile(.{ .sub_path = t.path, .data = body });
        const sig = try kp.sign(body, null);
        var b64: [88]u8 = undefined;
        const enc = std.base64.standard.Encoder.encode(&b64, &sig.toBytes());
        var line: [200]u8 = undefined;
        const sp = try std.fmt.allocPrint(std.testing.allocator, "{s}.sig", .{t.path});
        defer std.testing.allocator.free(sp);
        try std.fs.cwd().writeFile(.{ .sub_path = sp, .data = try std.fmt.bufPrint(&line, "{s} {s}\n", .{ key_id, enc }) });
    }
    fn table(t: *TestRelease, keys: []const Key) Table {
        return .{ .gpa = std.testing.allocator, .path = t.path, .keys = keys };
    }
};

fn release(comptime version: u64, comptime opus_input: []const u8) []const u8 {
    return "{\"format\":2,\"version\":" ++ std.fmt.comptimePrint("{d}", .{version}) ++ ",\"published\":\"2026-11-25\",\"rows\":[{\"provider\":\"anthropic\",\"model\":\"claude-opus-5-5\",\"input\":" ++ opus_input ++ ",\"output\":20}],\"multipliers\":[{\"provider\":\"anthropic\",\"models\":[\"claude-opus-5-5*\"],\"when\":\"inference_geo=us\",\"factor\":1.1}]}";
}

test "a signed release is taken, a tampered one keeps the last good table" {
    var t = try TestRelease.init();
    defer t.deinit();
    const keys = [_]Key{.{ .id = "silo-2026", .public = &t.pub_b64 }};
    var tab = t.table(&keys);
    defer tab.deinit();
    try t.publish(t.kp, "silo-2026", release(1, "4"));
    try std.testing.expectEqual(@as(f64, 4), tab.find("anthropic", "claude-opus-5-5", "standard", "2026-11-25").?.input);
    // Same signature, other bytes: a price lowered by hand.
    const tampered = release(2, "0.4");
    try std.fs.cwd().writeFile(.{ .sub_path = t.path, .data = tampered });
    try std.testing.expectEqual(@as(f64, 4), tab.find("anthropic", "claude-opus-5-5", "standard", "2026-11-25").?.input);
    // Signed by a key Silo does not hold.
    const other = try Ed25519.KeyPair.generateDeterministic([_]u8{9} ** 32);
    try t.publish(other, "silo-2026", release(3, "0.4"));
    try std.testing.expectEqual(@as(f64, 4), tab.find("anthropic", "claude-opus-5-5", "standard", "2026-11-25").?.input);
    try t.publish(other, "someone", release(3, "0.4"));
    try std.testing.expectEqual(@as(f64, 4), tab.find("anthropic", "claude-opus-5-5", "standard", "2026-11-25").?.input);
    // A real next release.
    try t.publish(t.kp, "silo-2026", release(4, "5"));
    try std.testing.expectEqual(@as(f64, 5), tab.find("anthropic", "claude-opus-5-5", "standard", "2026-11-25").?.input);
    try std.testing.expectEqual(@as(u64, 4), tab.version());
}

test "an older signed release is refused, also after a restart" {
    var t = try TestRelease.init();
    defer t.deinit();
    const keys = [_]Key{.{ .id = "silo-2026", .public = &t.pub_b64 }};
    {
        var tab = t.table(&keys);
        defer tab.deinit();
        try t.publish(t.kp, "silo-2026", release(5, "4"));
        try std.testing.expectEqual(@as(u64, 5), blk: {
            _ = tab.find("anthropic", "claude-opus-5-5", "standard", "2026-11-25");
            break :blk tab.version();
        });
    }
    // An older release, validly signed, replayed while Silo was stopped.
    try t.publish(t.kp, "silo-2026", release(3, "0.4"));
    var tab = t.table(&keys);
    defer tab.deinit();
    try std.testing.expect(tab.find("anthropic", "claude-opus-5-5", "standard", "2026-11-25") == null);
    // The same version with other bytes is refused too.
    try t.publish(t.kp, "silo-2026", release(5, "0.4"));
    try std.testing.expect(tab.find("anthropic", "claude-opus-5-5", "standard", "2026-11-25") == null);
    // The same release again is taken.
    try t.publish(t.kp, "silo-2026", release(5, "4"));
    try std.testing.expectEqual(@as(f64, 4), tab.find("anthropic", "claude-opus-5-5", "standard", "2026-11-25").?.input);
}

test "a second key, as for a rotation" {
    var t = try TestRelease.init();
    defer t.deinit();
    const next = try Ed25519.KeyPair.generateDeterministic([_]u8{11} ** 32);
    var next_b64: [44]u8 = undefined;
    _ = std.base64.standard.Encoder.encode(&next_b64, &next.public_key.toBytes());
    const keys = [_]Key{ .{ .id = "silo-2026", .public = &t.pub_b64 }, .{ .id = "silo-2027", .public = &next_b64 } };
    var tab = t.table(&keys);
    defer tab.deinit();
    try t.publish(next, "silo-2027", release(1, "4"));
    try std.testing.expectEqual(@as(f64, 4), tab.find("anthropic", "claude-opus-5-5", "standard", "2026-11-25").?.input);
}

test "the user's own rows win, and multipliers apply to what was asked" {
    var t = try TestRelease.init();
    defer t.deinit();
    const keys = [_]Key{.{ .id = "silo-2026", .public = &t.pub_b64 }};
    var tab = t.table(&keys);
    defer tab.deinit();
    try t.publish(t.kp, "silo-2026", release(1, "4"));
    const lp = try std.fmt.allocPrint(std.testing.allocator, "{s}/my-prices.json", .{t.root});
    defer std.testing.allocator.free(lp);
    try std.fs.cwd().writeFile(.{ .sub_path = lp, .data = "{\"rows\":[{\"provider\":\"anthropic\",\"model\":\"claude-opus-6-0\",\"input\":6,\"output\":30}],\"multipliers\":[{\"provider\":\"anthropic\",\"factor\":0.8}]}" });
    tab.local_path = lp;
    const day = "2026-11-25";
    try std.testing.expectEqual(@as(f64, 6), tab.find("anthropic", "claude-opus-6-0", "standard", day).?.input);
    const none = [_][]const u8{};
    const us = [_][]const u8{"inference_geo=us"};
    try std.testing.expectApproxEqAbs(@as(f64, 3.2), tab.findFor("anthropic", "claude-opus-5-5", "standard", &none, day).?.input, 1e-9);
    try std.testing.expectApproxEqAbs(@as(f64, 4 * 1.1 * 0.8), tab.findFor("anthropic", "claude-opus-5-5", "standard", &us, day).?.input, 1e-9);
    try std.testing.expectApproxEqAbs(@as(f64, 20 * 1.1 * 0.8), tab.findFor("anthropic", "claude-opus-5-5", "standard", &us, day).?.output, 1e-9);
}

test "a bound for a model with no row" {
    const rows = [_]Row{
        .{ .provider = "anthropic", .model = "claude-opus-5-5", .input = 4, .output = 20, .cache_write_long = 8 },
        .{ .provider = "anthropic", .model = "claude-fable-5-2", .input = 8, .output = 40, .cache_write_long = 16 },
        .{ .provider = "anthropic", .model = "claude-haiku-5-5", .input = 0.1, .output = 0.5, .long_above = 100000, .long_input_mult = 5, .long_output_mult = 5 },
        .{ .provider = "anthropic", .model = "claude-fable-5-2", .tier = "fast", .input = 16, .output = 80 },
        .{ .provider = "anthropic", .model = "claude-next", .from = "2027-01-01", .input = 100, .output = 500 },
    };
    var tab: Table = .{ .gpa = std.testing.allocator, .path = "/nonexistent" };
    var rows_copy = rows;
    tab.parsed = .{ .arena = undefined, .value = .{ .rows = &rows_copy } };
    const b = blk: {
        // `bound` refreshes; the file does not exist, so the rows above stay.
        tab.mu.lock();
        tab.stamp = stampOf("/nonexistent", null);
        tab.mu.unlock();
        break :blk tab.bound("anthropic", "standard", "2026-11-25", 2).?;
    };
    tab.parsed = null;
    try std.testing.expectEqual(@as(f64, 16), b.input);
    try std.testing.expectEqual(@as(f64, 80), b.output);
    try std.testing.expectEqual(@as(f64, 32), b.cache_write_long);
}

test "the version last taken is never seen half written (research/2026-11-25-durability-on-a-mac)" {
    // The rollback check reads `<file>.seen`. If a reader, or a start after a power cut, finds it
    // empty or cut, readSeen gives null and an older signed release is taken. Written in place
    // (truncate, then write), it is empty for a moment; written to a temporary file, flushed and
    // renamed, it is the old line or the new one.
    var t = try TestRelease.init();
    defer t.deinit();
    var tab = t.table(&.{});
    const Reader = struct {
        fn run(tb: *Table, stop: *std.atomic.Value(bool), torn: *std.atomic.Value(u32)) void {
            while (!stop.load(.acquire)) {
                var pbuf: [512]u8 = undefined;
                const sp = std.fmt.bufPrint(&pbuf, "{s}.seen", .{tb.path}) catch return;
                var buf: [128]u8 = undefined;
                const got = std.fs.cwd().readFile(sp, &buf) catch continue; // not there yet
                if (got.len == 0 or got[got.len - 1] != '\n') _ = torn.fetchAdd(1, .monotonic);
            }
        }
    };
    tab.writeSeen(1, [_]u8{1} ** 32);
    var stop = std.atomic.Value(bool).init(false);
    var torn = std.atomic.Value(u32).init(0);
    const th = try std.Thread.spawn(.{}, Reader.run, .{ &tab, &stop, &torn });
    for (0..3000) |i| tab.writeSeen(i + 2, [_]u8{@truncate(i)} ** 32);
    stop.store(true, .release);
    th.join();
    try std.testing.expectEqual(@as(u32, 0), torn.load(.monotonic));
    try std.testing.expectEqual(@as(u64, 3001), tab.readSeen().?.version);
}
