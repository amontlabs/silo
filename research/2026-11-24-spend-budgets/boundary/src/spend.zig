//! What the boundary does with one model call for budgets: which computer and person it is for,
//! whether its credential is one Silo gave, what it will cost at most before it leaves, whether
//! it may leave, what it cost once it ends, and the refusal a program shows when it may not.
const std = @import("std");
const usage = @import("usage.zig");
const prices = @import("prices.zig");
const budget = @import("budget.zig");
const Allocator = std.mem.Allocator;

pub const Credential = struct {
    value: []const u8,
    /// "key": billed per call (an API key). "plan": a subscription's sign-in.
    kind: []const u8 = "key",
};

pub const Computer = struct {
    name: []const u8,
    /// The address the computer's programs reach the boundary at. In the experiments, each
    /// computer's /etc/hosts points the providers' hosts at its own loopback address.
    address: []const u8,
    person: ?[]const u8 = null,
    /// The computer this one was forked from. With no limits of its own, a fork spends from its
    /// parent's budget (`budget-of-a-fork`), and its conversations follow `fork_conversations`.
    fork_of: ?[]const u8 = null,
    credentials: []Credential = &.{},
    limits: budget.Limits = .{},
};

pub const Person = struct { name: []const u8, limits: budget.Limits = .{} };

pub const Config = struct {
    policy: []const u8 = "finish",
    prices: []const u8 = "prices.json",
    /// Where what each budget spent is kept, so that it holds across a restart, a crash or a
    /// power cut (journal.zig). Unset, the ledgers live in memory and start at zero.
    journal: ?[]const u8 = null,
    /// With keys, the price table must be a release signed by one of them, newer than every one
    /// taken before (research/2026-11-25-model-prices).
    price_keys: []prices.Key = &.{},
    /// The user's own rows and multipliers, unsigned, which win over the release.
    local_prices: ?[]const u8 = null,
    /// A model with no price, under a budget. "refuse" (#374). "bound": admitted and counted at
    /// the dearest row of its provider and tier, times `unknown_margin`, marked as a bound.
    unknown_model: []const u8 = "refuse",
    unknown_margin: f64 = 2,
    /// On a host Silo reads, a request whose credential is not one this computer was given is
    /// refused (#351's first recommendation).
    deny_unknown_credentials: bool = true,
    /// Bytes per token for a conversation the boundary has not seen a reply of yet.
    cold_bytes_per_token: f64 = 3.0,
    /// How the input of a call is priced before it leaves. "dearest": every token at the
    /// dearest rate it can incur (a bound). "likely": the part an earlier request of the same
    /// conversation sent at the cache-read rate, the rest at the dearest (an estimate, which a
    /// cache that expired makes too low).
    input_price: []const u8 = "dearest",
    /// While a call runs, count its output by time as well: elapsed seconds times this many
    /// tokens a second, up to `max_tokens`, when that is more than the visible output. Hidden
    /// reasoning then counts while it happens. 0 counts visible output only.
    output_tps: f64 = 0,
    /// For `reserve_typical`: the output held before the provider's own counts give a better
    /// figure (the 90th percentile of the model's settled calls).
    typical_output_tokens: i64 = 8000,
    /// How a refusal by a rate is sent: "400" like any refusal, which makes the program stop, or
    /// "429" with `retry-after` until the window passes again, which programs may wait out.
    rate_refusal: []const u8 = "400",
    /// Known model hosts the boundary cannot count. Refused while a budget covers the computer.
    uncountable_hosts: [][]const u8 = &.{},
    /// A streamed Chat Completions call that does not ask for its usage
    /// (`stream_options.include_usage`) ends with no count. "estimate": it leaves and is settled
    /// on an estimate. "deny": under a budget it is refused, and the refusal says what to set.
    chat_without_usage: []const u8 = "estimate",
    /// Work that runs at the provider after the connection ends: background responses, batches
    /// and Gemini's cache storage. "pass": as #374, nothing is counted for it. "hold": it is
    /// admitted only if its bound fits, and holds that bound until its result passes the boundary.
    out_of_sight: []const u8 = "pass",
    /// A budget reached while calls stream. "finish": they finish. "cut": once what is counted
    /// passes `cut_share` of a total, each call it covers is ended, and the program gets an error
    /// event in the provider's shape.
    mid_stream: []const u8 = "finish",
    cut_share: f64 = 1.0,
    /// With `input_price` "likely": an earlier request's cache is taken as gone once its lifetime
    /// has passed, and its part is priced as a write again.
    cache_ttl_aware: bool = false,
    /// OpenAI's prompt cache lifetime, which its replies do not say. 5 minutes is the least.
    openai_cache_ttl_s: f64 = 300,
    /// The stand-in providers run their clocks faster, so that an hour of cache passes in seconds.
    /// The boundary multiplies the providers' lifetimes by this. 1 for real providers.
    provider_time_scale: f64 = 1,
    /// A fork's first request that continues one of its parent's conversations: "fresh" starts a
    /// conversation at zero (#373's text), "carry" starts it at what the parent's had spent, and
    /// "shared" spends from the parent's conversation itself.
    fork_conversations: []const u8 = "fresh",
    computers: []Computer = &.{},
    people: []Person = &.{},
};

pub const Provider = enum { anthropic, openai, google };

pub fn providerOf(host: []const u8) ?Provider {
    if (eq(host, "api.anthropic.com")) return .anthropic;
    if (eq(host, "api.openai.com") or eq(host, "chatgpt.com")) return .openai;
    if (eq(host, "generativelanguage.googleapis.com")) return .google;
    return null;
}

pub fn providerName(p: Provider) []const u8 {
    return @tagName(p);
}

/// Which model call format a request is, from its path. Null: not a model call, not metered.
pub fn formatOf(p: Provider, method: []const u8, path: []const u8) ?usage.Format {
    if (!eq(method, "POST")) return null;
    const q = std.mem.indexOfScalar(u8, path, '?') orelse path.len;
    const bare = path[0..q];
    switch (p) {
        .anthropic => if (eq(bare, "/v1/messages")) return .anthropic,
        .openai => {
            if (std.mem.endsWith(u8, bare, "/responses")) return .responses;
            if (std.mem.endsWith(u8, bare, "/chat/completions")) return .chat;
        },
        .google => if (std.mem.endsWith(u8, bare, ":generateContent") or std.mem.endsWith(u8, bare, ":streamGenerateContent")) return .gemini,
    }
    return null;
}

/// What the boundary reads from a model request before it leaves.
pub const Request = struct {
    model: []u8,
    /// The most output the call may produce, or a default when the request sets none.
    max_tokens: i64,
    tier: []const u8,
    body_len: usize,
    stream: bool = false,
    /// Chat Completions: the request asks for the usage chunk at the end of its stream.
    include_usage: bool = false,
    /// Chat Completions: the number of choices, each written and billed in full.
    n: i64 = 1,
    /// OpenAI Responses: the state at the provider this request continues, and whether it runs
    /// in the background.
    previous_response_id: ?[]u8 = null,
    conversation: ?[]u8 = null,
    background: bool = false,
    /// What the request asks that changes its price by a multiplier: "inference_geo=us".
    asked: []const []const u8 = &.{},
    /// A hash of the conversation up to each of its items (messages, input items, contents), in
    /// order. A later request of the same conversation starts with the same items, so the last
    /// one of an earlier request is one of these. Markers of the prompt cache, which move from
    /// one request to the next, are left out.
    prefixes: []u64,

    pub fn deinit(self: *Request, gpa: Allocator) void {
        gpa.free(self.model);
        gpa.free(self.prefixes);
        if (self.previous_response_id) |x| gpa.free(x);
        if (self.conversation) |x| gpa.free(x);
    }

    /// The hash of the whole conversation this request sends.
    pub fn whole(self: *const Request) u64 {
        return if (self.prefixes.len > 0) self.prefixes[self.prefixes.len - 1] else 0;
    }
};

const geo_us = [_][]const u8{"inference_geo=us"};

pub fn readRequest(gpa: Allocator, format: usage.Format, path: []const u8, body: []const u8) !Request {
    var parsed = try std.json.parseFromSlice(std.json.Value, gpa, body, .{});
    defer parsed.deinit();
    if (parsed.value != .object) return error.NotAnObject;
    const o = parsed.value.object;
    var model: []const u8 = usage.str(o.get("model")) orelse "";
    var max: ?i64 = null;
    var tier: []const u8 = "standard";
    var items: ?std.json.Array = null;
    var n: i64 = 1;
    var include_usage = false;
    var prev: ?[]const u8 = null;
    var conv: ?[]const u8 = null;
    var background = false;
    var asked: []const []const u8 = &.{};
    switch (format) {
        .anthropic => {
            max = usage.int(o.get("max_tokens"));
            if (eq(usage.str(o.get("speed")) orelse "", "fast")) tier = "fast";
            if (eq(usage.str(o.get("inference_geo")) orelse "", "us")) asked = &geo_us;
            items = usage.arr(o.get("messages"));
        },
        .responses => {
            max = usage.int(o.get("max_output_tokens"));
            if (usage.str(o.get("previous_response_id"))) |x| prev = x;
            if (usage.str(o.get("conversation"))) |x| conv = x;
            if (usage.obj(o.get("conversation"))) |co| conv = usage.str(co.get("id"));
            background = boolOf(o.get("background"));
            tier = tierOf(usage.str(o.get("service_tier")));
            items = usage.arr(o.get("input"));
        },
        .chat => {
            max = usage.int(o.get("max_completion_tokens")) orelse usage.int(o.get("max_tokens"));
            n = usage.int(o.get("n")) orelse 1;
            if (usage.obj(o.get("stream_options"))) |so| include_usage = boolOf(so.get("include_usage"));
            tier = tierOf(usage.str(o.get("service_tier")));
            items = usage.arr(o.get("messages"));
        },
        .gemini => {
            // The model is in the path: /v1beta/models/<model>:streamGenerateContent
            if (std.mem.indexOf(u8, path, "/models/")) |i| {
                const rest = path[i + 8 ..];
                model = rest[0 .. std.mem.indexOfScalar(u8, rest, ':') orelse rest.len];
            }
            if (usage.obj(o.get("generationConfig"))) |g| max = usage.int(g.get("maxOutputTokens"));
            items = usage.arr(o.get("contents"));
        },
    }
    var prefixes: std.ArrayList(u64) = .empty;
    errdefer prefixes.deinit(gpa);
    var h = std.hash.Wyhash.init(0);
    h.update(model);
    if (items) |a| for (a.items) |*it| {
        normalize(it);
        var w: std.Io.Writer.Allocating = .init(gpa);
        defer w.deinit();
        std.json.Stringify.value(it.*, .{}, &w.writer) catch {};
        h.update(w.written());
        var copy = h;
        try prefixes.append(gpa, copy.final());
    };
    return .{
        .model = try gpa.dupe(u8, model),
        // A request with no limit can produce up to the model's own; 64k stands in for it here.
        .max_tokens = max orelse 64000,
        .tier = tier,
        .body_len = body.len,
        .stream = boolOf(o.get("stream")),
        .include_usage = include_usage,
        .n = @max(1, n),
        .previous_response_id = if (prev) |x| try gpa.dupe(u8, x) else null,
        .conversation = if (conv) |x| try gpa.dupe(u8, x) else null,
        .background = background,
        .asked = asked,
        .prefixes = try prefixes.toOwnedSlice(gpa),
    };
}

fn boolOf(v: ?std.json.Value) bool {
    const x = v orelse return false;
    return x == .bool and x.bool;
}

fn tierOf(s: ?[]const u8) []const u8 {
    const t = s orelse return "standard";
    if (eq(t, "priority") or eq(t, "fast")) return "priority";
    if (eq(t, "flex")) return "flex";
    return "standard";
}

/// Takes out what changes in an item from one request to the next while the item stays the
/// same: the prompt cache's markers, and a content of one text block written as a list or as a
/// plain string.
fn normalize(v: *std.json.Value) void {
    switch (v.*) {
        .object => |*o| {
            _ = o.orderedRemove("cache_control");
            if (o.getPtr("content")) |c| if (c.* == .array and c.array.items.len == 1) {
                const b = c.array.items[0];
                if (b == .object and eq(usage.str(b.object.get("type")) orelse "", "text") and b.object.count() == 2) {
                    if (b.object.get("text")) |t| c.* = t;
                }
            };
            var it = o.iterator();
            while (it.next()) |e| normalize(e.value_ptr);
        },
        .array => |*a| for (a.items) |*x| normalize(x),
        else => {},
    }
}

/// The credential a request carries, as sent: an API key header, a bearer token, or Google's key
/// in the query.
pub fn credentialOf(head: []const u8, path: []const u8) ?[]const u8 {
    var it = std.mem.splitSequence(u8, head, "\r\n");
    _ = it.next();
    while (it.next()) |line| {
        const colon = std.mem.indexOfScalar(u8, line, ':') orelse continue;
        const name = std.mem.trim(u8, line[0..colon], " ");
        const value = std.mem.trim(u8, line[colon + 1 ..], " \t");
        if (std.ascii.eqlIgnoreCase(name, "x-api-key") or std.ascii.eqlIgnoreCase(name, "x-goog-api-key")) return value;
        if (std.ascii.eqlIgnoreCase(name, "authorization")) {
            if (std.ascii.startsWithIgnoreCase(value, "bearer ")) return std.mem.trim(u8, value[7..], " ");
            return value;
        }
    }
    if (std.mem.indexOf(u8, path, "key=")) |i| {
        const rest = path[i + 4 ..];
        return rest[0 .. std.mem.indexOfScalar(u8, rest, '&') orelse rest.len];
    }
    return null;
}

/// A header's value in a head, or null.
pub fn header(head: []const u8, want: []const u8) ?[]const u8 {
    var it = std.mem.splitSequence(u8, head, "\r\n");
    _ = it.next();
    while (it.next()) |line| {
        const colon = std.mem.indexOfScalar(u8, line, ':') orelse continue;
        if (std.ascii.eqlIgnoreCase(std.mem.trim(u8, line[0..colon], " "), want)) return std.mem.trim(u8, line[colon + 1 ..], " \t");
    }
    return null;
}

/// The plan windows a reply's headers report for the whole account, as shares from 0 to 1.
pub fn planWindows(head: []const u8) struct { five_hour: ?f64, seven_day: ?f64 } {
    var five: ?f64 = null;
    var seven: ?f64 = null;
    if (header(head, "anthropic-ratelimit-unified-5h-utilization")) |v| five = std.fmt.parseFloat(f64, v) catch null;
    if (header(head, "anthropic-ratelimit-unified-7d-utilization")) |v| seven = std.fmt.parseFloat(f64, v) catch null;
    if (header(head, "x-codex-primary-used-percent")) |v| five = (std.fmt.parseFloat(f64, v) catch 0) / 100;
    if (header(head, "x-codex-secondary-used-percent")) |v| seven = (std.fmt.parseFloat(f64, v) catch 0) / 100;
    return .{ .five_hour = five, .seven_day = seven };
}

/// The refusal a program shows: HTTP 400 in the provider's own error shape (#369 found it is
/// the one status every program it tried stops on and prints).
pub fn refusal(gpa: Allocator, format: usage.Format, message: []const u8) ![]u8 {
    var buf: std.Io.Writer.Allocating = .init(gpa);
    defer buf.deinit();
    const w = &buf.writer;
    switch (format) {
        .anthropic => try w.print("{{\"type\":\"error\",\"error\":{{\"type\":\"invalid_request_error\",\"message\":{f}}}}}", .{std.json.fmt(message, .{})}),
        .responses, .chat => try w.print("{{\"error\":{{\"message\":{f},\"type\":\"invalid_request_error\",\"param\":null,\"code\":\"silo_budget_reached\"}}}}", .{std.json.fmt(message, .{})}),
        .gemini => try w.print("{{\"error\":{{\"code\":400,\"message\":{f},\"status\":\"INVALID_ARGUMENT\"}}}}", .{std.json.fmt(message, .{})}),
    }
    const body = buf.written();
    return std.fmt.allocPrint(gpa, "HTTP/1.1 400 Bad Request\r\ncontent-type: application/json\r\ncontent-length: {d}\r\nx-should-retry: false\r\nx-silo-refused: budget\r\n\r\n{s}", .{ body.len, body });
}

/// A refusal that asks the program to wait: HTTP 429 in the provider's rate-limit shape, with
/// `retry-after` in seconds.
pub fn wait(gpa: Allocator, format: usage.Format, message: []const u8, secs: u64) ![]u8 {
    var buf: std.Io.Writer.Allocating = .init(gpa);
    defer buf.deinit();
    const w = &buf.writer;
    switch (format) {
        .anthropic => try w.print("{{\"type\":\"error\",\"error\":{{\"type\":\"rate_limit_error\",\"message\":{f}}}}}", .{std.json.fmt(message, .{})}),
        .responses, .chat => try w.print("{{\"error\":{{\"message\":{f},\"type\":\"requests\",\"param\":null,\"code\":\"rate_limit_exceeded\"}}}}", .{std.json.fmt(message, .{})}),
        .gemini => try w.print("{{\"error\":{{\"code\":429,\"message\":{f},\"status\":\"RESOURCE_EXHAUSTED\"}}}}", .{std.json.fmt(message, .{})}),
    }
    const body = buf.written();
    return std.fmt.allocPrint(gpa, "HTTP/1.1 429 Too Many Requests\r\ncontent-type: application/json\r\ncontent-length: {d}\r\nretry-after: {d}\r\nx-silo-refused: rate\r\n\r\n{s}", .{ body.len, secs, body });
}

/// The event that ends a stream Silo cut, in the provider's own shape, so the program shows why.
pub fn cutEvent(gpa: Allocator, format: usage.Format, message: []const u8) ![]u8 {
    return switch (format) {
        .anthropic => std.fmt.allocPrint(gpa, "event: error\ndata: {{\"type\":\"error\",\"error\":{{\"type\":\"invalid_request_error\",\"message\":{f}}}}}\n\n", .{std.json.fmt(message, .{})}),
        .responses => std.fmt.allocPrint(gpa, "event: error\ndata: {{\"type\":\"error\",\"code\":\"silo_budget_reached\",\"message\":{f},\"param\":null}}\n\n", .{std.json.fmt(message, .{})}),
        .chat => std.fmt.allocPrint(gpa, "data: {{\"error\":{{\"message\":{f},\"type\":\"invalid_request_error\",\"code\":\"silo_budget_reached\"}}}}\n\n", .{std.json.fmt(message, .{})}),
        .gemini => std.fmt.allocPrint(gpa, "data: {{\"error\":{{\"code\":400,\"message\":{f},\"status\":\"INVALID_ARGUMENT\"}}}}\n\n", .{std.json.fmt(message, .{})}),
    };
}

pub fn providerKey(format: usage.Format) []const u8 {
    return switch (format) {
        .anthropic => "anthropic",
        .responses, .chat => "openai",
        .gemini => "google",
    };
}

/// The cost of a call's counts, each attempt at its own model's price. Null when a model of it
/// has no price.
pub fn costOf(table: *prices.Table, provider: []const u8, m: *const usage.Meter, asked: []const []const u8, day: []const u8) ?f64 {
    const tier = tierFromReply(m);
    if (m.parts.items.len > 0) {
        var total: f64 = 0;
        for (m.parts.items) |p| {
            const r = table.findFor(provider, p.model, tier, asked, day) orelse return null;
            total += prices.cost(r, p.counts);
        }
        return total;
    }
    const r = table.findFor(provider, m.model orelse return null, tier, asked, day) orelse return null;
    return prices.cost(r, m.counts);
}

fn tierFromReply(m: *const usage.Meter) []const u8 {
    const t = m.tier orelse return "standard";
    if (eq(t, "fast")) return "fast";
    if (eq(t, "priority")) return "priority";
    if (eq(t, "flex")) return "flex";
    return "standard";
}

fn eq(a: []const u8, b: []const u8) bool {
    return std.mem.eql(u8, a, b);
}

test "formats from paths" {
    try std.testing.expectEqual(usage.Format.anthropic, formatOf(.anthropic, "POST", "/v1/messages?beta=true").?);
    try std.testing.expect(formatOf(.anthropic, "POST", "/v1/messages/count_tokens") == null);
    try std.testing.expectEqual(usage.Format.responses, formatOf(.openai, "POST", "/backend-api/codex/responses").?);
    try std.testing.expectEqual(usage.Format.chat, formatOf(.openai, "POST", "/v1/chat/completions").?);
    try std.testing.expectEqual(usage.Format.gemini, formatOf(.google, "POST", "/v1beta/models/gemini-3.1-pro-preview:streamGenerateContent?alt=sse").?);
}

test "credentials and plan windows from heads" {
    const h = "POST /v1/messages HTTP/1.1\r\nHost: api.anthropic.com\r\nAuthorization: Bearer sk-ant-oat01-x\r\n\r\n";
    try std.testing.expectEqualStrings("sk-ant-oat01-x", credentialOf(h, "/v1/messages").?);
    try std.testing.expectEqualStrings("AIza1", credentialOf("POST / HTTP/1.1\r\n\r\n", "/v1beta/models/g:generateContent?key=AIza1&alt=sse").?);
    const r = "HTTP/1.1 200 OK\r\nanthropic-ratelimit-unified-5h-utilization: 0.25\r\nanthropic-ratelimit-unified-7d-utilization: 0.5\r\n\r\n";
    const p = planWindows(r);
    try std.testing.expectEqual(@as(f64, 0.25), p.five_hour.?);
    try std.testing.expectEqual(@as(f64, 0.5), planWindows("HTTP/1.1 200 OK\r\nx-codex-secondary-used-percent: 50\r\n\r\n").seven_day.?);
}

test "request: model, limit, tier and a conversation name that survives growth" {
    const gpa = std.testing.allocator;
    var a = try readRequest(gpa, .anthropic, "/v1/messages", "{\"model\":\"claude-opus-5-5\",\"max_tokens\":32000,\"speed\":\"fast\",\"system\":\"s\",\"messages\":[{\"role\":\"user\",\"content\":\"go\"}]}");
    defer a.deinit(gpa);
    var b = try readRequest(gpa, .anthropic, "/v1/messages", "{\"model\":\"claude-opus-5-5\",\"max_tokens\":32000,\"system\":\"s\",\"messages\":[{\"role\":\"user\",\"content\":\"go\"},{\"role\":\"assistant\",\"content\":\"ok\"}]}");
    defer b.deinit(gpa);
    try std.testing.expectEqualStrings("fast", a.tier);
    try std.testing.expectEqual(@as(i64, 32000), a.max_tokens);
    // The second request's first item is the first request's whole conversation.
    try std.testing.expectEqual(a.whole(), b.prefixes[0]);
    var g = try readRequest(gpa, .gemini, "/v1beta/models/gemini-3.8-flash:streamGenerateContent?alt=sse", "{\"contents\":[{\"parts\":[{\"text\":\"hi\"}]}],\"generationConfig\":{\"maxOutputTokens\":100}}");
    defer g.deinit(gpa);
    try std.testing.expectEqualStrings("gemini-3.8-flash", g.model);
    try std.testing.expectEqual(@as(i64, 100), g.max_tokens);
}

test "request: chat usage and choices, responses state and background" {
    const gpa = std.testing.allocator;
    var a = try readRequest(gpa, .chat, "/v1/chat/completions", "{\"model\":\"m\",\"stream\":true,\"n\":4,\"stream_options\":{\"include_usage\":true},\"messages\":[]}");
    defer a.deinit(gpa);
    try std.testing.expect(a.stream and a.include_usage);
    try std.testing.expectEqual(@as(i64, 4), a.n);
    var b = try readRequest(gpa, .responses, "/v1/responses", "{\"model\":\"m\",\"background\":true,\"previous_response_id\":\"resp_9\",\"conversation\":{\"id\":\"conv_1\"},\"input\":[]}");
    defer b.deinit(gpa);
    try std.testing.expect(b.background);
    try std.testing.expectEqualStrings("resp_9", b.previous_response_id.?);
    try std.testing.expectEqualStrings("conv_1", b.conversation.?);
}

test "refusal in each provider's shape" {
    const gpa = std.testing.allocator;
    const a = try refusal(gpa, .anthropic, "Silo: no");
    defer gpa.free(a);
    try std.testing.expect(std.mem.indexOf(u8, a, "\"type\":\"invalid_request_error\",\"message\":\"Silo: no\"") != null);
    const g = try refusal(gpa, .gemini, "Silo: no");
    defer gpa.free(g);
    try std.testing.expect(std.mem.indexOf(u8, g, "\"status\":\"INVALID_ARGUMENT\"") != null);
}
