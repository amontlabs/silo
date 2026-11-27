//! Pure, incremental readers for what the boundary sees of a model's reply: HTTP chunked framing,
//! server-sent events, and a watcher per provider that tells when a tool call starts and when it is
//! whole. They never touch the bytes that are forwarded: they read a copy.
const std = @import("std");
const Allocator = std.mem.Allocator;

/// Decodes `Transfer-Encoding: chunked`. Feed raw bytes, get the body bytes appended to `out`.
pub const Chunked = struct {
    state: enum { size, size_ext, size_lf, data, data_cr, data_lf, trailer, trailer_lf, done } = .size,
    remaining: u64 = 0,
    saw_digit: bool = false,
    trailer_empty: bool = true,

    pub fn feed(self: *Chunked, gpa: Allocator, in: []const u8, out: *std.ArrayList(u8)) !void {
        var i: usize = 0;
        while (i < in.len) {
            const c = in[i];
            switch (self.state) {
                .size => {
                    if (std.fmt.charToDigit(c, 16)) |d| {
                        if (self.remaining > (std.math.maxInt(u64) >> 4)) return error.ChunkTooLarge;
                        self.remaining = self.remaining * 16 + d;
                        self.saw_digit = true;
                    } else |_| {
                        if (!self.saw_digit) return error.BadChunk;
                        if (c == ';' or c == ' ' or c == '\t') self.state = .size_ext else if (c == '\r') self.state = .size_lf else return error.BadChunk;
                    }
                    i += 1;
                },
                .size_ext => {
                    if (c == '\r') self.state = .size_lf;
                    i += 1;
                },
                .size_lf => {
                    if (c != '\n') return error.BadChunk;
                    i += 1;
                    self.saw_digit = false;
                    if (self.remaining == 0) {
                        self.state = .trailer;
                        self.trailer_empty = true;
                    } else self.state = .data;
                },
                .data => {
                    const n: usize = @intCast(@min(self.remaining, in.len - i));
                    try out.appendSlice(gpa, in[i .. i + n]);
                    self.remaining -= n;
                    i += n;
                    if (self.remaining == 0) self.state = .data_cr;
                },
                .data_cr => {
                    if (c != '\r') return error.BadChunk;
                    self.state = .data_lf;
                    i += 1;
                },
                .data_lf => {
                    if (c != '\n') return error.BadChunk;
                    self.state = .size;
                    i += 1;
                },
                .trailer => {
                    if (c == '\r') self.state = .trailer_lf else self.trailer_empty = false;
                    i += 1;
                },
                .trailer_lf => {
                    if (c != '\n') return error.BadChunk;
                    i += 1;
                    if (self.trailer_empty) self.state = .done else {
                        self.state = .trailer;
                        self.trailer_empty = true;
                    }
                },
                .done => return error.DataAfterEnd,
            }
        }
    }

    pub fn finished(self: *const Chunked) bool {
        return self.state == .done;
    }
};

/// Splits a text/event-stream into events. An event's data lines are joined with '\n'.
pub const Sse = struct {
    line: std.ArrayList(u8) = .empty,
    event: std.ArrayList(u8) = .empty,
    data: std.ArrayList(u8) = .empty,
    has_data: bool = false,
    max_event: usize = 16 << 20,

    pub const Event = struct { name: []const u8, data: []const u8 };

    pub fn deinit(self: *Sse, gpa: Allocator) void {
        self.line.deinit(gpa);
        self.event.deinit(gpa);
        self.data.deinit(gpa);
    }

    /// Calls `sink.onEvent(Event)` for each complete event.
    pub fn feed(self: *Sse, gpa: Allocator, in: []const u8, sink: anytype) !void {
        for (in) |c| {
            if (c == '\n') {
                var l = self.line.items;
                if (l.len > 0 and l[l.len - 1] == '\r') l = l[0 .. l.len - 1];
                try self.takeLine(gpa, l, sink);
                self.line.clearRetainingCapacity();
            } else {
                if (self.line.items.len >= self.max_event) return error.EventTooLarge;
                try self.line.append(gpa, c);
            }
        }
    }

    fn takeLine(self: *Sse, gpa: Allocator, l: []const u8, sink: anytype) !void {
        if (l.len == 0) {
            if (self.has_data or self.event.items.len > 0) {
                try sink.onEvent(.{ .name = self.event.items, .data = self.data.items });
            }
            self.event.clearRetainingCapacity();
            self.data.clearRetainingCapacity();
            self.has_data = false;
            return;
        }
        if (l[0] == ':') return;
        const colon = std.mem.indexOfScalar(u8, l, ':');
        const field = if (colon) |k| l[0..k] else l;
        var value: []const u8 = if (colon) |k| l[k + 1 ..] else "";
        if (value.len > 0 and value[0] == ' ') value = value[1..];
        if (std.mem.eql(u8, field, "event")) {
            self.event.clearRetainingCapacity();
            try self.event.appendSlice(gpa, value);
        } else if (std.mem.eql(u8, field, "data")) {
            if (self.data.items.len + value.len > self.max_event) return error.EventTooLarge;
            if (self.has_data) try self.data.append(gpa, '\n');
            try self.data.appendSlice(gpa, value);
            self.has_data = true;
        }
    }
};

pub const Provider = enum { anthropic, openai };

/// One tool call the model asked for, once it is whole.
pub const ToolCall = struct {
    id: []const u8,
    name: []const u8,
    input: []const u8,
};

/// Watches the events of one reply. `open` counts the tool calls that have started and are not
/// whole yet: while it is above zero, the boundary holds the reply back.
pub const Watcher = struct {
    gpa: Allocator,
    provider: Provider,
    open: u32 = 0,
    events: u32 = 0,
    // Anthropic: the content block index of each open tool call, with its parts.
    cur_index: ?i64 = null,
    cur_id: std.ArrayList(u8) = .empty,
    cur_name: std.ArrayList(u8) = .empty,
    cur_input: std.ArrayList(u8) = .empty,
    done: std.ArrayList(ToolCall) = .empty,
    input_tokens: ?i64 = null,
    output_tokens: ?i64 = null,
    model: ?[]u8 = null,
    malformed: u32 = 0,

    pub fn init(gpa: Allocator, provider: Provider) Watcher {
        return .{ .gpa = gpa, .provider = provider };
    }

    pub fn deinit(self: *Watcher) void {
        self.cur_id.deinit(self.gpa);
        self.cur_name.deinit(self.gpa);
        self.cur_input.deinit(self.gpa);
        for (self.done.items) |t| {
            self.gpa.free(t.id);
            self.gpa.free(t.name);
            self.gpa.free(t.input);
        }
        self.done.deinit(self.gpa);
        if (self.model) |m| self.gpa.free(m);
    }

    pub fn onEvent(self: *Watcher, ev: Sse.Event) !void {
        self.events += 1;
        if (ev.data.len == 0 or ev.data[0] != '{') return;
        var parsed = std.json.parseFromSlice(std.json.Value, self.gpa, ev.data, .{}) catch {
            self.malformed += 1;
            return;
        };
        defer parsed.deinit();
        if (parsed.value != .object) return;
        const o = parsed.value.object;
        const typ = str(o.get("type")) orelse return;
        switch (self.provider) {
            .anthropic => try self.anthropic(typ, o),
            .openai => try self.openai(typ, o),
        }
    }

    fn anthropic(self: *Watcher, typ: []const u8, o: std.json.ObjectMap) !void {
        if (eq(typ, "message_start")) {
            if (obj(o.get("message"))) |m| {
                if (str(m.get("model"))) |s| self.setModel(s);
                if (obj(m.get("usage"))) |u| self.input_tokens = int(u.get("input_tokens"));
            }
        } else if (eq(typ, "content_block_start")) {
            const cb = obj(o.get("content_block")) orelse return;
            const t = str(cb.get("type")) orelse return;
            if (eq(t, "tool_use") or eq(t, "server_tool_use")) {
                if (self.cur_index != null) return error.NestedToolCall;
                self.cur_index = int(o.get("index")) orelse return error.NoIndex;
                try reset(self.gpa, &self.cur_id, str(cb.get("id")) orelse "");
                try reset(self.gpa, &self.cur_name, str(cb.get("name")) orelse "");
                self.cur_input.clearRetainingCapacity();
                self.open += 1;
            }
        } else if (eq(typ, "content_block_delta")) {
            const idx = int(o.get("index")) orelse return;
            if (self.cur_index == null or self.cur_index.? != idx) return;
            const d = obj(o.get("delta")) orelse return;
            if (str(d.get("partial_json"))) |p| try self.cur_input.appendSlice(self.gpa, p);
        } else if (eq(typ, "content_block_stop")) {
            const idx = int(o.get("index")) orelse return;
            if (self.cur_index == null or self.cur_index.? != idx) return;
            try self.finish(self.cur_id.items, self.cur_name.items, self.cur_input.items);
            self.cur_index = null;
        } else if (eq(typ, "message_delta")) {
            if (obj(o.get("usage"))) |u| self.output_tokens = int(u.get("output_tokens"));
        }
    }

    fn openai(self: *Watcher, typ: []const u8, o: std.json.ObjectMap) !void {
        if (eq(typ, "response.output_item.added")) {
            const it = obj(o.get("item")) orelse return;
            const t = str(it.get("type")) orelse return;
            if (isCall(t)) self.open += 1;
        } else if (eq(typ, "response.output_item.done")) {
            const it = obj(o.get("item")) orelse return;
            const t = str(it.get("type")) orelse return;
            if (isCall(t)) {
                if (self.open == 0) return error.CallNotStarted;
                const input = str(it.get("arguments")) orelse str(it.get("input")) orelse "";
                try self.finish(str(it.get("call_id")) orelse "", str(it.get("name")) orelse t, input);
            }
        } else if (eq(typ, "response.completed") or eq(typ, "response.created")) {
            const r = obj(o.get("response")) orelse return;
            if (str(r.get("model"))) |s| self.setModel(s);
            if (obj(r.get("usage"))) |u| {
                self.input_tokens = int(u.get("input_tokens"));
                self.output_tokens = int(u.get("output_tokens"));
            }
        }
    }

    fn isCall(t: []const u8) bool {
        return eq(t, "function_call") or eq(t, "custom_tool_call") or eq(t, "local_shell_call");
    }

    fn finish(self: *Watcher, id: []const u8, name: []const u8, input: []const u8) !void {
        try self.done.append(self.gpa, .{
            .id = try self.gpa.dupe(u8, id),
            .name = try self.gpa.dupe(u8, name),
            .input = try self.gpa.dupe(u8, input),
        });
        self.open -= 1;
    }

    fn setModel(self: *Watcher, s: []const u8) void {
        if (self.model) |m| self.gpa.free(m);
        self.model = self.gpa.dupe(u8, s) catch null;
    }
};

fn reset(gpa: Allocator, l: *std.ArrayList(u8), s: []const u8) !void {
    l.clearRetainingCapacity();
    try l.appendSlice(gpa, s);
}
fn eq(a: []const u8, b: []const u8) bool {
    return std.mem.eql(u8, a, b);
}
fn str(v: ?std.json.Value) ?[]const u8 {
    const x = v orelse return null;
    return if (x == .string) x.string else null;
}
fn obj(v: ?std.json.Value) ?std.json.ObjectMap {
    const x = v orelse return null;
    return if (x == .object) x.object else null;
}
fn int(v: ?std.json.Value) ?i64 {
    const x = v orelse return null;
    return if (x == .integer) x.integer else null;
}

/// The ids of the tool results a request carries, so that the boundary knows when the program
/// has answered a call it denied.
pub fn toolResultIds(gpa: Allocator, provider: Provider, body: []const u8) !std.ArrayList([]u8) {
    var ids: std.ArrayList([]u8) = .empty;
    var parsed = std.json.parseFromSlice(std.json.Value, gpa, body, .{}) catch return ids;
    defer parsed.deinit();
    if (parsed.value != .object) return ids;
    const o = parsed.value.object;
    switch (provider) {
        .anthropic => {
            const msgs = o.get("messages") orelse return ids;
            if (msgs != .array or msgs.array.items.len == 0) return ids;
            const last = msgs.array.items[msgs.array.items.len - 1];
            if (last != .object) return ids;
            const content = last.object.get("content") orelse return ids;
            if (content != .array) return ids;
            for (content.array.items) |c| {
                if (c != .object) continue;
                if (!eq(str(c.object.get("type")) orelse "", "tool_result")) continue;
                if (str(c.object.get("tool_use_id"))) |id| try ids.append(gpa, try gpa.dupe(u8, id));
            }
        },
        .openai => {
            const input = o.get("input") orelse return ids;
            if (input != .array) return ids;
            for (input.array.items) |c| {
                if (c != .object) continue;
                const t = str(c.object.get("type")) orelse continue;
                if (!eq(t, "function_call_output") and !eq(t, "custom_tool_call_output")) continue;
                if (str(c.object.get("call_id"))) |id| try ids.append(gpa, try gpa.dupe(u8, id));
            }
        },
    }
    return ids;
}

// ---- tests ----

const Collect = struct {
    gpa: Allocator,
    names: std.ArrayList([]u8) = .empty,
    fn onEvent(self: *Collect, ev: Sse.Event) !void {
        try self.names.append(self.gpa, try std.fmt.allocPrint(self.gpa, "{s}|{s}", .{ ev.name, ev.data }));
    }
};

test "chunked, split at every byte" {
    const gpa = std.testing.allocator;
    const raw = "5\r\nhello\r\n7;x=y\r\n, world\r\n0\r\nT: v\r\n\r\n";
    var c: Chunked = .{};
    var out: std.ArrayList(u8) = .empty;
    defer out.deinit(gpa);
    for (raw) |b| try c.feed(gpa, &[_]u8{b}, &out);
    try std.testing.expectEqualStrings("hello, world", out.items);
    try std.testing.expect(c.finished());
}

test "sse events across arbitrary splits" {
    const gpa = std.testing.allocator;
    const raw = "event: a\ndata: {\"x\":1}\n\n: comment\r\nevent: b\r\ndata: l1\r\ndata: l2\r\n\r\n";
    var k: usize = 1;
    while (k < raw.len) : (k += 1) {
        var s: Sse = .{};
        defer s.deinit(gpa);
        var col: Collect = .{ .gpa = gpa };
        defer {
            for (col.names.items) |n| gpa.free(n);
            col.names.deinit(gpa);
        }
        try s.feed(gpa, raw[0..k], &col);
        try s.feed(gpa, raw[k..], &col);
        try std.testing.expectEqual(@as(usize, 2), col.names.items.len);
        try std.testing.expectEqualStrings("a|{\"x\":1}", col.names.items[0]);
        try std.testing.expectEqualStrings("b|l1\nl2", col.names.items[1]);
    }
}

test "anthropic watcher holds from tool_use start to its stop" {
    const gpa = std.testing.allocator;
    var w = Watcher.init(gpa, .anthropic);
    defer w.deinit();
    var s: Sse = .{};
    defer s.deinit(gpa);
    try s.feed(gpa, "event: content_block_start\ndata: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_1\",\"name\":\"Bash\",\"input\":{}}}\n\n", &w);
    try std.testing.expectEqual(@as(u32, 1), w.open);
    try s.feed(gpa, "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\":\"}}\n\n", &w);
    try s.feed(gpa, "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"ls\\\"}\"}}\n\n", &w);
    try std.testing.expectEqual(@as(u32, 1), w.open);
    try s.feed(gpa, "event: content_block_stop\ndata: {\"type\":\"content_block_stop\",\"index\":1}\n\n", &w);
    try std.testing.expectEqual(@as(u32, 0), w.open);
    try std.testing.expectEqualStrings("{\"command\":\"ls\"}", w.done.items[0].input);
    try std.testing.expectEqualStrings("toolu_1", w.done.items[0].id);
}

test "tool result ids from requests" {
    const gpa = std.testing.allocator;
    var a = try toolResultIds(gpa, .anthropic, "{\"messages\":[{\"role\":\"user\",\"content\":[{\"type\":\"tool_result\",\"tool_use_id\":\"toolu_9\",\"content\":\"x\"}]}]}");
    defer {
        for (a.items) |x| gpa.free(x);
        a.deinit(gpa);
    }
    try std.testing.expectEqualStrings("toolu_9", a.items[0]);
    var b = try toolResultIds(gpa, .openai, "{\"input\":[{\"type\":\"function_call_output\",\"call_id\":\"call_3\",\"output\":\"y\"}]}");
    defer {
        for (b.items) |x| gpa.free(x);
        b.deinit(gpa);
    }
    try std.testing.expectEqualStrings("call_3", b.items[0]);
}

// Fuzz entry: no input may crash the readers or leak.
test "fuzz the readers" {
    try std.testing.fuzz({}, fuzzOne, .{});
}

fn fuzzOne(_: void, input: []const u8) anyerror!void {
    const gpa = std.testing.allocator;
    var c: Chunked = .{};
    var out: std.ArrayList(u8) = .empty;
    defer out.deinit(gpa);
    c.feed(gpa, input, &out) catch {};
    inline for (.{ Provider.anthropic, Provider.openai }) |p| {
        var w = Watcher.init(gpa, p);
        defer w.deinit();
        var s: Sse = .{};
        defer s.deinit(gpa);
        s.feed(gpa, input, &w) catch {};
        if (toolResultIds(gpa, p, input)) |ids_| {
            var ids = ids_;
            for (ids.items) |x| gpa.free(x);
            ids.deinit(gpa);
        } else |_| {}
    }
}
