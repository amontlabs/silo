//! Reads what a model call costs from a copy of its reply: the token counts each provider reports,
//! and while the reply streams, how much visible output has come. One `Meter` per model call. It
//! never touches the bytes that are forwarded.
const std = @import("std");
const Allocator = std.mem.Allocator;

/// The wire formats Silo reads. One host can carry several (api.openai.com: Responses and Chat
/// Completions), so the format is told by the request's path, not the host.
pub const Format = enum { anthropic, responses, chat, gemini };

/// Token counts of one model run, split the way prices are. `input` is the input billed at the
/// base rate: never the cached part, never the part written to the cache.
pub const Counts = struct {
    input: i64 = 0,
    cache_read: i64 = 0,
    /// A short-lived cache write: Anthropic's 5 minutes, OpenAI's 30 minutes.
    cache_write_short: i64 = 0,
    /// A long-lived cache write: Anthropic's 1 hour, OpenAI's 12 hours.
    cache_write_long: i64 = 0,
    /// All output tokens, reasoning included (every provider bills reasoning as output).
    output: i64 = 0,
    /// The reasoning part of `output`, when the provider says it. Shown, not priced twice.
    reasoning: i64 = 0,
    web_searches: i64 = 0,

    pub fn totalInput(self: Counts) i64 {
        return self.input + self.cache_read + self.cache_write_short + self.cache_write_long;
    }
    pub fn add(self: *Counts, o: Counts) void {
        self.input += o.input;
        self.cache_read += o.cache_read;
        self.cache_write_short += o.cache_write_short;
        self.cache_write_long += o.cache_write_long;
        self.output += o.output;
        self.reasoning += o.reasoning;
        self.web_searches += o.web_searches;
    }
};

/// One attempt inside a call that the provider bills at its own model's rate: Anthropic lists a
/// server-side fallback or the advisor in `usage.iterations`.
pub const Part = struct { model: []u8, counts: Counts };

pub const Meter = struct {
    gpa: Allocator,
    format: Format,
    model: ?[]u8 = null,
    /// The counts at the top level of the reply.
    counts: Counts = .{},
    /// The counts per attempt, when the reply lists them. They replace `counts` when present.
    parts: std.ArrayList(Part) = .empty,
    /// "fast" or "standard" (Anthropic `usage.speed`), or the tier OpenAI and Google say ran.
    tier: ?[]u8 = null,
    /// The provider's final count arrived: the call can be settled on it.
    final: bool = false,
    /// Anthropic gives the input before any output: the call's input is known while it runs.
    input_known: bool = false,
    /// Characters of visible output (text, tool call arguments, thinking text) seen so far.
    visible_chars: u64 = 0,
    events: u32 = 0,
    malformed: u32 = 0,
    error_status: bool = false,
    /// OpenAI Responses: the response's id, which a later request names in
    /// `previous_response_id`, and its status. A background response answers `queued` or
    /// `in_progress` at once and runs on at the provider: its counts come later, by its id.
    resp_id: ?[]u8 = null,
    status: ?[]u8 = null,
    /// Searches the provider ran and bills by the thousand, outside the token counts: OpenAI's
    /// `web_search_call` items, Gemini's grounding queries.
    searches: i64 = 0,

    pub fn init(gpa: Allocator, format: Format) Meter {
        return .{ .gpa = gpa, .format = format };
    }

    pub fn deinit(self: *Meter) void {
        if (self.model) |m| self.gpa.free(m);
        if (self.tier) |t| self.gpa.free(t);
        if (self.resp_id) |t| self.gpa.free(t);
        if (self.status) |t| self.gpa.free(t);
        for (self.parts.items) |p| self.gpa.free(p.model);
        self.parts.deinit(self.gpa);
    }

    /// Sets the model from the request, for formats whose reply may not name it (Gemini names it
    /// in the path). A model named by the reply replaces it.
    pub fn setModel(self: *Meter, s: []const u8) void {
        if (self.model) |m| self.gpa.free(m);
        self.model = self.gpa.dupe(u8, s) catch null;
    }

    fn setTier(self: *Meter, s: []const u8) void {
        if (self.tier) |t| self.gpa.free(t);
        self.tier = self.gpa.dupe(u8, s) catch null;
    }

    fn setOwned(self: *Meter, field: *?[]u8, s: []const u8) void {
        if (field.*) |t| self.gpa.free(t);
        field.* = self.gpa.dupe(u8, s) catch null;
    }

    /// A Responses reply that said the work goes on at the provider: no count yet.
    pub fn pending(self: *const Meter) bool {
        const st = self.status orelse return false;
        return eq(st, "queued") or eq(st, "in_progress");
    }

    /// The counts the call is settled on: the attempts when listed, else the top level.
    pub fn billed(self: *const Meter) Counts {
        if (self.parts.items.len == 0) return self.counts;
        var c: Counts = .{};
        for (self.parts.items) |p| c.add(p.counts);
        return c;
    }

    /// One server-sent event, or one WebSocket message, of the reply.
    pub fn onEvent(self: *Meter, ev: anytype) !void {
        self.events += 1;
        try self.onJson(ev.data);
    }

    /// A body that is not streamed: one JSON value, or for Gemini an array of them.
    pub fn onBody(self: *Meter, body: []const u8) !void {
        var parsed = std.json.parseFromSlice(std.json.Value, self.gpa, body, .{}) catch {
            self.malformed += 1;
            return;
        };
        defer parsed.deinit();
        switch (parsed.value) {
            .array => |a| for (a.items) |v| try self.onValue(v, true),
            else => try self.onValue(parsed.value, true),
        }
    }

    fn onJson(self: *Meter, data: []const u8) !void {
        if (data.len == 0 or (data[0] != '{' and data[0] != '[')) return;
        var parsed = std.json.parseFromSlice(std.json.Value, self.gpa, data, .{}) catch {
            self.malformed += 1;
            return;
        };
        defer parsed.deinit();
        try self.onValue(parsed.value, false);
    }

    fn onValue(self: *Meter, v: std.json.Value, whole: bool) !void {
        if (v != .object) return;
        const o = v.object;
        switch (self.format) {
            .anthropic => try self.anthropic(o, whole),
            .responses => try self.responses(o, whole),
            .chat => try self.chat(o),
            .gemini => try self.gemini(o),
        }
    }

    // ---- Anthropic Messages ----

    fn anthropic(self: *Meter, o: std.json.ObjectMap, whole: bool) !void {
        const typ = str(o.get("type")) orelse return;
        if (whole and eq(typ, "message")) {
            // A reply that was not streamed is the message itself.
            if (str(o.get("model"))) |s| self.setModel(s);
            if (obj(o.get("usage"))) |u| try self.anthropicUsage(u);
            self.input_known = true;
            self.final = true;
            self.visible_chars += contentChars(o.get("content"));
            return;
        }
        if (eq(typ, "error")) {
            self.error_status = true;
            return;
        }
        if (eq(typ, "message_start")) {
            const m = obj(o.get("message")) orelse return;
            if (str(m.get("model"))) |s| self.setModel(s);
            if (obj(m.get("usage"))) |u| try self.anthropicUsage(u);
            self.input_known = true;
        } else if (eq(typ, "content_block_delta")) {
            const d = obj(o.get("delta")) orelse return;
            inline for (.{ "text", "partial_json", "thinking" }) |k| {
                if (str(d.get(k))) |s| self.visible_chars += s.len;
            }
        } else if (eq(typ, "message_delta")) {
            // The counts here are cumulative: they replace what came before, field by field.
            if (obj(o.get("usage"))) |u| try self.anthropicUsage(u);
        } else if (eq(typ, "message_stop")) {
            self.final = true;
        }
    }

    fn anthropicUsage(self: *Meter, u: std.json.ObjectMap) !void {
        anthropicCounts(u, &self.counts);
        if (str(u.get("speed"))) |s| self.setTier(s);
        if (arr(u.get("iterations"))) |its| {
            for (self.parts.items) |p| self.gpa.free(p.model);
            self.parts.clearRetainingCapacity();
            for (its.items) |it| {
                if (it != .object) continue;
                var c: Counts = .{};
                anthropicCounts(it.object, &c);
                const m = str(it.object.get("model")) orelse self.model orelse "";
                try self.parts.append(self.gpa, .{ .model = try self.gpa.dupe(u8, m), .counts = c });
            }
        }
    }

    // ---- OpenAI Responses (HTTP or WebSocket) ----

    fn responses(self: *Meter, o: std.json.ObjectMap, whole: bool) !void {
        if (whole and eq(str(o.get("object")) orelse "", "response")) {
            self.responseObject(o);
            self.final = !self.pending();
            return;
        }
        const typ = str(o.get("type")) orelse return;
        if (eq(typ, "response.created") or eq(typ, "response.in_progress")) {
            if (obj(o.get("response"))) |r| {
                if (str(r.get("model"))) |s| self.setModel(s);
                if (str(r.get("id"))) |s| self.setOwned(&self.resp_id, s);
            }
        } else if (std.mem.endsWith(u8, typ, ".delta")) {
            if (str(o.get("delta"))) |s| self.visible_chars += s.len;
        } else if (eq(typ, "response.completed") or eq(typ, "response.incomplete") or eq(typ, "response.failed")) {
            if (obj(o.get("response"))) |r| self.responseObject(r);
            self.final = true;
        } else if (eq(typ, "error")) {
            self.error_status = true;
        }
    }

    fn responseObject(self: *Meter, r: std.json.ObjectMap) void {
        if (str(r.get("model"))) |s| self.setModel(s);
        if (str(r.get("service_tier"))) |s| self.setTier(s);
        if (str(r.get("id"))) |s| self.setOwned(&self.resp_id, s);
        if (str(r.get("status"))) |s| self.setOwned(&self.status, s);
        var searches: i64 = 0;
        if (arr(r.get("output"))) |items| for (items.items) |it| {
            if (it == .object and eq(str(it.object.get("type")) orelse "", "web_search_call")) searches += 1;
        };
        self.searches = searches;
        const u = obj(r.get("usage")) orelse return;
        var c: Counts = .{ .web_searches = searches };
        const total_in = int(u.get("input_tokens")) orelse 0;
        if (obj(u.get("input_tokens_details"))) |d| {
            c.cache_read = int(d.get("cached_tokens")) orelse 0;
            const w = int(d.get("cache_write_tokens")) orelse 0;
            var long: i64 = 0;
            if (obj(d.get("cache_write_tokens_details"))) |wd| long = int(wd.get("ttl_12h_tokens")) orelse 0;
            c.cache_write_long = long;
            c.cache_write_short = w - long;
        }
        c.input = total_in - c.cache_read - c.cache_write_short - c.cache_write_long;
        c.output = int(u.get("output_tokens")) orelse 0;
        if (obj(u.get("output_tokens_details"))) |d| c.reasoning = int(d.get("reasoning_tokens")) orelse 0;
        self.counts = c;
        self.input_known = true;
    }

    // ---- OpenAI Chat Completions ----

    fn chat(self: *Meter, o: std.json.ObjectMap) !void {
        if (str(o.get("model"))) |s| {
            if (self.model == null) self.setModel(s);
        }
        if (arr(o.get("choices"))) |cs| for (cs.items) |ch| {
            if (ch != .object) continue;
            const d = obj(ch.object.get("delta")) orelse obj(ch.object.get("message")) orelse continue;
            if (str(d.get("content"))) |s| self.visible_chars += s.len;
            if (str(d.get("reasoning_content"))) |s| self.visible_chars += s.len;
            if (arr(d.get("tool_calls"))) |tcs| for (tcs.items) |tc| {
                if (tc != .object) continue;
                if (obj(tc.object.get("function"))) |f| if (str(f.get("arguments"))) |s| {
                    self.visible_chars += s.len;
                };
            };
        };
        if (str(o.get("service_tier"))) |s| self.setTier(s);
        const u = obj(o.get("usage")) orelse return;
        var c: Counts = .{};
        const p = int(u.get("prompt_tokens")) orelse 0;
        if (obj(u.get("prompt_tokens_details"))) |d| c.cache_read = int(d.get("cached_tokens")) orelse 0;
        c.input = p - c.cache_read;
        c.output = int(u.get("completion_tokens")) orelse 0;
        if (obj(u.get("completion_tokens_details"))) |d| c.reasoning = int(d.get("reasoning_tokens")) orelse 0;
        self.counts = c;
        self.input_known = true;
        self.final = true;
    }

    // ---- Google Gemini ----

    fn gemini(self: *Meter, o: std.json.ObjectMap) !void {
        if (obj(o.get("error")) != null) {
            self.error_status = true;
            return;
        }
        if (str(o.get("modelVersion"))) |s| self.setModel(s);
        if (arr(o.get("candidates"))) |cs| for (cs.items) |cand| {
            if (cand != .object) continue;
            if (obj(cand.object.get("content"))) |content| if (arr(content.get("parts"))) |ps| for (ps.items) |p| {
                if (p != .object) continue;
                if (str(p.object.get("text"))) |s| self.visible_chars += s.len;
                if (p.object.get("functionCall")) |fc| self.visible_chars += jsonLen(fc);
            };
            if (str(cand.object.get("finishReason")) != null) self.final = true;
            if (obj(cand.object.get("groundingMetadata"))) |g| if (arr(g.get("webSearchQueries"))) |q| {
                self.searches = @intCast(q.items.len);
            };
        };
        // Each chunk may carry usageMetadata; the last one has the totals. Take the latest.
        const u = obj(o.get("usageMetadata")) orelse return;
        var c: Counts = .{};
        const prompt = int(u.get("promptTokenCount")) orelse 0;
        const tool_prompt = int(u.get("toolUsePromptTokenCount")) orelse 0;
        c.cache_read = int(u.get("cachedContentTokenCount")) orelse 0;
        c.input = prompt + tool_prompt - c.cache_read;
        const cand_out = int(u.get("candidatesTokenCount")) orelse 0;
        c.reasoning = int(u.get("thoughtsTokenCount")) orelse 0;
        c.output = cand_out + c.reasoning;
        c.web_searches = self.searches;
        self.counts = c;
        self.input_known = true;
        if (str(u.get("serviceTier"))) |s| self.setTier(s);
    }
};

fn anthropicCounts(u: std.json.ObjectMap, c: *Counts) void {
    if (int(u.get("input_tokens"))) |v| c.input = v;
    if (int(u.get("cache_read_input_tokens"))) |v| c.cache_read = v;
    if (int(u.get("output_tokens"))) |v| c.output = v;
    // The split by lifetime, when given, is exact. Otherwise every write is taken as 5 minutes.
    if (obj(u.get("cache_creation"))) |cc| {
        c.cache_write_short = int(cc.get("ephemeral_5m_input_tokens")) orelse 0;
        c.cache_write_long = int(cc.get("ephemeral_1h_input_tokens")) orelse 0;
    } else if (int(u.get("cache_creation_input_tokens"))) |v| {
        c.cache_write_short = v;
        c.cache_write_long = 0;
    }
    if (obj(u.get("server_tool_use"))) |s| {
        if (int(s.get("web_search_requests"))) |v| c.web_searches = v;
    }
}

fn contentChars(v: ?std.json.Value) u64 {
    const a = arr(v) orelse return 0;
    var n: u64 = 0;
    for (a.items) |b| {
        if (b != .object) continue;
        if (str(b.object.get("text"))) |s| n += s.len;
        if (b.object.get("input")) |i| n += jsonLen(i);
    }
    return n;
}

fn jsonLen(v: std.json.Value) u64 {
    var w = std.Io.Writer.Discarding.init(&.{});
    std.json.Stringify.value(v, .{}, &w.writer) catch return 0;
    return w.fullCount();
}

fn eq(a: []const u8, b: []const u8) bool {
    return std.mem.eql(u8, a, b);
}
pub fn str(v: ?std.json.Value) ?[]const u8 {
    const x = v orelse return null;
    return if (x == .string) x.string else null;
}
pub fn obj(v: ?std.json.Value) ?std.json.ObjectMap {
    const x = v orelse return null;
    return if (x == .object) x.object else null;
}
pub fn arr(v: ?std.json.Value) ?std.json.Array {
    const x = v orelse return null;
    return if (x == .array) x.array else null;
}
pub fn int(v: ?std.json.Value) ?i64 {
    const x = v orelse return null;
    return switch (x) {
        .integer => |i| i,
        .float => |f| @intFromFloat(f),
        else => null,
    };
}

// ---- tests ----

const Ev = struct { data: []const u8 };

test "anthropic: input at start, cumulative counts at the end, cache split by lifetime" {
    const gpa = std.testing.allocator;
    var m = Meter.init(gpa, .anthropic);
    defer m.deinit();
    try m.onEvent(Ev{ .data = "{\"type\":\"message_start\",\"message\":{\"model\":\"claude-opus-5-5\",\"usage\":{\"input_tokens\":12,\"cache_creation_input_tokens\":3000,\"cache_creation\":{\"ephemeral_5m_input_tokens\":1000,\"ephemeral_1h_input_tokens\":2000},\"cache_read_input_tokens\":40000,\"output_tokens\":1}}}" });
    try std.testing.expect(m.input_known);
    try std.testing.expectEqual(@as(i64, 43012), m.counts.totalInput());
    try m.onEvent(Ev{ .data = "{\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"hello\"}}" });
    try m.onEvent(Ev{ .data = "{\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":510,\"server_tool_use\":{\"web_search_requests\":1}}}" });
    try std.testing.expect(!m.final);
    try m.onEvent(Ev{ .data = "{\"type\":\"message_stop\"}" });
    try std.testing.expect(m.final);
    const c = m.billed();
    try std.testing.expectEqual(@as(i64, 12), c.input);
    try std.testing.expectEqual(@as(i64, 1000), c.cache_write_short);
    try std.testing.expectEqual(@as(i64, 2000), c.cache_write_long);
    try std.testing.expectEqual(@as(i64, 40000), c.cache_read);
    try std.testing.expectEqual(@as(i64, 510), c.output);
    try std.testing.expectEqual(@as(i64, 1), c.web_searches);
    try std.testing.expectEqual(@as(u64, 5), m.visible_chars);
    try std.testing.expectEqualStrings("claude-opus-5-5", m.model.?);
}

test "anthropic: iterations replace the top level" {
    const gpa = std.testing.allocator;
    var m = Meter.init(gpa, .anthropic);
    defer m.deinit();
    try m.onEvent(Ev{ .data = "{\"type\":\"message_delta\",\"usage\":{\"input_tokens\":412,\"output_tokens\":264,\"iterations\":[{\"type\":\"message\",\"model\":\"claude-fable-5\",\"input_tokens\":535,\"output_tokens\":0},{\"type\":\"fallback_message\",\"model\":\"claude-opus-4-8\",\"input_tokens\":412,\"output_tokens\":264}]}}" });
    try std.testing.expectEqual(@as(usize, 2), m.parts.items.len);
    try std.testing.expectEqual(@as(i64, 947), m.billed().input);
    try std.testing.expectEqualStrings("claude-fable-5", m.parts.items[0].model);
}

test "responses: usage only at the end, cached and written parts taken out of input" {
    const gpa = std.testing.allocator;
    var m = Meter.init(gpa, .responses);
    defer m.deinit();
    try m.onEvent(Ev{ .data = "{\"type\":\"response.created\",\"response\":{\"model\":\"gpt-6.2-sol\"}}" });
    try std.testing.expect(!m.input_known);
    try m.onEvent(Ev{ .data = "{\"type\":\"response.output_text.delta\",\"delta\":\"abc\"}" });
    try m.onEvent(Ev{ .data = "{\"type\":\"response.completed\",\"response\":{\"model\":\"gpt-6.2-sol\",\"service_tier\":\"default\",\"usage\":{\"input_tokens\":10000,\"input_tokens_details\":{\"cached_tokens\":6000,\"cache_write_tokens\":3000,\"cache_write_tokens_details\":{\"ttl_30m_tokens\":1000,\"ttl_12h_tokens\":2000}},\"output_tokens\":900,\"output_tokens_details\":{\"reasoning_tokens\":700}}}}" });
    const c = m.billed();
    try std.testing.expect(m.final);
    try std.testing.expectEqual(@as(i64, 1000), c.input);
    try std.testing.expectEqual(@as(i64, 6000), c.cache_read);
    try std.testing.expectEqual(@as(i64, 1000), c.cache_write_short);
    try std.testing.expectEqual(@as(i64, 2000), c.cache_write_long);
    try std.testing.expectEqual(@as(i64, 900), c.output);
    try std.testing.expectEqual(@as(i64, 700), c.reasoning);
    try std.testing.expectEqual(@as(u64, 3), m.visible_chars);
}

test "chat: no usage unless the last chunk carries it" {
    const gpa = std.testing.allocator;
    var m = Meter.init(gpa, .chat);
    defer m.deinit();
    try m.onEvent(Ev{ .data = "{\"model\":\"gpt-6.2-sol\",\"choices\":[{\"delta\":{\"content\":\"hi there\"}}],\"usage\":null}" });
    try std.testing.expect(!m.final);
    try m.onEvent(Ev{ .data = "{\"model\":\"gpt-6.2-sol\",\"choices\":[],\"usage\":{\"prompt_tokens\":100,\"prompt_tokens_details\":{\"cached_tokens\":64},\"completion_tokens\":30,\"completion_tokens_details\":{\"reasoning_tokens\":20}}}" });
    try std.testing.expect(m.final);
    try std.testing.expectEqual(@as(i64, 36), m.billed().input);
    try std.testing.expectEqual(@as(i64, 30), m.billed().output);
    try std.testing.expectEqual(@as(u64, 8), m.visible_chars);
}

test "gemini: the latest usageMetadata wins, thoughts are output" {
    const gpa = std.testing.allocator;
    var m = Meter.init(gpa, .gemini);
    defer m.deinit();
    m.setModel("gemini-3.1-pro-preview");
    try m.onEvent(Ev{ .data = "{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ab\"}]}}],\"usageMetadata\":{\"promptTokenCount\":500,\"cachedContentTokenCount\":200}}" });
    try m.onEvent(Ev{ .data = "{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"cd\"}]},\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":500,\"cachedContentTokenCount\":200,\"candidatesTokenCount\":40,\"thoughtsTokenCount\":300,\"toolUsePromptTokenCount\":10},\"modelVersion\":\"gemini-3.1-pro-preview\"}" });
    const c = m.billed();
    try std.testing.expect(m.final);
    try std.testing.expectEqual(@as(i64, 310), c.input);
    try std.testing.expectEqual(@as(i64, 200), c.cache_read);
    try std.testing.expectEqual(@as(i64, 340), c.output);
    try std.testing.expectEqual(@as(i64, 300), c.reasoning);
}

test "gemini: a body that is not streamed is an array of chunks" {
    const gpa = std.testing.allocator;
    var m = Meter.init(gpa, .gemini);
    defer m.deinit();
    try m.onBody("[{\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"a\"}]}}]},{\"candidates\":[{\"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":9,\"candidatesTokenCount\":3}}]");
    try std.testing.expect(m.final);
    try std.testing.expectEqual(@as(i64, 9), m.billed().input);
}

test "responses: a background response is not final until it ends, and searches count" {
    const gpa = std.testing.allocator;
    var m = Meter.init(gpa, .responses);
    defer m.deinit();
    try m.onBody("{\"id\":\"resp_1\",\"object\":\"response\",\"status\":\"queued\",\"model\":\"gpt-6.2-sol\",\"output\":[]}");
    try std.testing.expect(!m.final);
    try std.testing.expect(m.pending());
    try std.testing.expectEqualStrings("resp_1", m.resp_id.?);
    var d = Meter.init(gpa, .responses);
    defer d.deinit();
    try d.onBody("{\"id\":\"resp_1\",\"object\":\"response\",\"status\":\"completed\",\"output\":[{\"type\":\"web_search_call\"},{\"type\":\"web_search_call\"}],\"usage\":{\"input_tokens\":10,\"output_tokens\":5}}");
    try std.testing.expect(d.final);
    try std.testing.expectEqual(@as(i64, 2), d.billed().web_searches);
}

test "gemini: grounding queries are searches" {
    const gpa = std.testing.allocator;
    var m = Meter.init(gpa, .gemini);
    defer m.deinit();
    try m.onBody("{\"candidates\":[{\"finishReason\":\"STOP\",\"groundingMetadata\":{\"webSearchQueries\":[\"a\",\"b\"]}}],\"usageMetadata\":{\"promptTokenCount\":9,\"candidatesTokenCount\":3}}");
    try std.testing.expectEqual(@as(i64, 2), m.billed().web_searches);
}
