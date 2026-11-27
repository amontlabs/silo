//! Spend that happens at the provider after the program's connection ends: background responses,
//! batches, and Gemini's explicit cache, billed as storage for as long as it lives. The boundary
//! sees the request that starts the work and, if the program fetches it through the boundary,
//! the result with its counts. This file reads both; main.zig holds the work between them.
const std = @import("std");
const usage = @import("usage.zig");
const prices = @import("prices.zig");
const spend = @import("spend.zig");
const Allocator = std.mem.Allocator;

/// What a request that is not itself a model call does to spend.
pub const Kind = enum {
    none,
    /// POST /v1/files: the input of an OpenAI batch, when its purpose is "batch".
    openai_file_upload,
    /// POST /v1/batches: starts a batch from an uploaded file.
    openai_batch_create,
    /// GET /v1/batches/{id}: names the file the results are in once it ends.
    openai_batch_get,
    /// GET /v1/files/{id}/content: the results, one JSON line per request.
    openai_file_content,
    /// POST /v1/messages/batches: an Anthropic batch, its requests inline.
    anthropic_batch_create,
    /// GET /v1/messages/batches/{id}/results.
    anthropic_batch_results,
    /// GET /v1/responses/{id}: a stored or background response, with its usage once it ends.
    response_get,
    /// POST /v1/responses/{id}/cancel.
    response_cancel,
    /// POST /v1beta/cachedContents, PATCH and DELETE /v1beta/cachedContents/{id}.
    gemini_cache_create,
    gemini_cache_patch,
    gemini_cache_delete,
};

pub fn classify(p: spend.Provider, method: []const u8, path: []const u8) Kind {
    const bare = path[0 .. std.mem.indexOfScalar(u8, path, '?') orelse path.len];
    const post = eq(method, "POST");
    const get = eq(method, "GET");
    switch (p) {
        .openai => {
            if (post and eq(bare, "/v1/files")) return .openai_file_upload;
            if (post and eq(bare, "/v1/batches")) return .openai_batch_create;
            if (get and std.mem.startsWith(u8, bare, "/v1/batches/") and segments(bare) == 3) return .openai_batch_get;
            if (get and std.mem.startsWith(u8, bare, "/v1/files/") and std.mem.endsWith(u8, bare, "/content")) return .openai_file_content;
            if (get and std.mem.startsWith(u8, bare, "/v1/responses/") and segments(bare) == 3) return .response_get;
            if (post and std.mem.startsWith(u8, bare, "/v1/responses/") and std.mem.endsWith(u8, bare, "/cancel")) return .response_cancel;
        },
        .anthropic => {
            if (post and eq(bare, "/v1/messages/batches")) return .anthropic_batch_create;
            if (get and std.mem.startsWith(u8, bare, "/v1/messages/batches/") and std.mem.endsWith(u8, bare, "/results")) return .anthropic_batch_results;
        },
        .google => {
            if (post and std.mem.endsWith(u8, bare, "/cachedContents")) return .gemini_cache_create;
            if (std.mem.indexOf(u8, bare, "/cachedContents/") != null) {
                if (eq(method, "PATCH")) return .gemini_cache_patch;
                if (eq(method, "DELETE")) return .gemini_cache_delete;
            }
        },
    }
    return .none;
}

fn segments(p: []const u8) usize {
    var n: usize = 0;
    var it = std.mem.tokenizeScalar(u8, p, '/');
    while (it.next()) |_| n += 1;
    return n;
}

/// The id a path names at `index` (counted from 0 after the leading slash).
pub fn pathPart(path: []const u8, index: usize) ?[]const u8 {
    const bare = path[0 .. std.mem.indexOfScalar(u8, path, '?') orelse path.len];
    var it = std.mem.tokenizeScalar(u8, bare, '/');
    var i: usize = 0;
    while (it.next()) |s| : (i += 1) if (i == index) return s;
    return null;
}

/// The bound of a batch: each request's input at the dearest batch rate, from its size, and its
/// whole output limit, times its choices. A request with no output limit is held at 64,000
/// tokens, as for a single call.
pub const Bound = struct {
    bound: f64 = 0,
    requests: u32 = 0,
    /// Requests whose model has no batch price: under a budget the batch cannot be counted.
    unpriced: u32 = 0,
    model: []const u8 = "",
};

pub fn boundOfRequest(table: *prices.Table, provider: []const u8, format: usage.Format, params: []const u8, day: []const u8, bytes_per_token: f64, out: *Bound) void {
    var parsed = std.json.parseFromSlice(std.json.Value, std.heap.c_allocator, params, .{}) catch {
        out.unpriced += 1;
        return;
    };
    defer parsed.deinit();
    if (parsed.value != .object) return;
    const o = parsed.value.object;
    const model = usage.str(o.get("model")) orelse "";
    const max: i64 = switch (format) {
        .anthropic => usage.int(o.get("max_tokens")),
        .responses => usage.int(o.get("max_output_tokens")),
        .chat => usage.int(o.get("max_completion_tokens")) orelse usage.int(o.get("max_tokens")),
        .gemini => if (usage.obj(o.get("generationConfig"))) |g| usage.int(g.get("maxOutputTokens")) else null,
    } orelse 64000;
    const n: i64 = @max(1, usage.int(o.get("n")) orelse 1);
    out.requests += 1;
    const r = table.find(provider, model, "batch", day) orelse {
        out.unpriced += 1;
        return;
    };
    const tokens: i64 = @intFromFloat(@as(f64, @floatFromInt(params.len)) / bytes_per_token);
    out.bound += (@as(f64, @floatFromInt(tokens)) * prices.dearestInput(r, tokens) + @as(f64, @floatFromInt(max * n)) * prices.outputRate(r, tokens)) / 1e6;
}

/// An OpenAI batch input file, from the multipart body of its upload: its JSONL, if its purpose is
/// "batch".
pub fn batchFile(body: []const u8, content_type: []const u8) ?[]const u8 {
    const i = std.mem.indexOf(u8, content_type, "boundary=") orelse return null;
    const bnd = std.mem.trim(u8, content_type[i + 9 ..], "\" ");
    var buf: [256]u8 = undefined;
    const delim = std.fmt.bufPrint(&buf, "--{s}", .{bnd}) catch return null;
    var file: ?[]const u8 = null;
    var is_batch = false;
    var it = std.mem.splitSequence(u8, body, delim);
    while (it.next()) |part| {
        const sep = std.mem.indexOf(u8, part, "\r\n\r\n") orelse continue;
        const head = part[0..sep];
        var data = part[sep + 4 ..];
        if (std.mem.endsWith(u8, data, "\r\n")) data = data[0 .. data.len - 2];
        if (std.mem.indexOf(u8, head, "name=\"purpose\"") != null) is_batch = std.mem.eql(u8, std.mem.trim(u8, data, " \r\n"), "batch");
        if (std.mem.indexOf(u8, head, "name=\"file\"") != null) file = data;
    }
    return if (is_batch) file else null;
}

/// The bound of every request in an OpenAI batch file.
pub fn boundOfOpenAIFile(table: *prices.Table, jsonl: []const u8, day: []const u8, bytes_per_token: f64) Bound {
    var out: Bound = .{};
    var it = std.mem.splitScalar(u8, jsonl, '\n');
    while (it.next()) |line| {
        if (std.mem.trim(u8, line, " \r").len == 0) continue;
        var parsed = std.json.parseFromSlice(std.json.Value, std.heap.c_allocator, line, .{}) catch {
            out.unpriced += 1;
            continue;
        };
        defer parsed.deinit();
        if (parsed.value != .object) continue;
        const url = usage.str(parsed.value.object.get("url")) orelse "";
        const format: usage.Format = if (std.mem.endsWith(u8, url, "/responses")) .responses else .chat;
        const b = parsed.value.object.get("body") orelse continue;
        var w: std.Io.Writer.Allocating = .init(std.heap.c_allocator);
        defer w.deinit();
        std.json.Stringify.value(b, .{}, &w.writer) catch continue;
        boundOfRequest(table, "openai", format, w.written(), day, bytes_per_token, &out);
    }
    return out;
}

/// The bound of an Anthropic batch, whose requests come inline.
pub fn boundOfAnthropicBatch(table: *prices.Table, body: []const u8, day: []const u8, bytes_per_token: f64) Bound {
    var out: Bound = .{};
    var parsed = std.json.parseFromSlice(std.json.Value, std.heap.c_allocator, body, .{}) catch return .{ .unpriced = 1 };
    defer parsed.deinit();
    const reqs = usage.arr(if (parsed.value == .object) parsed.value.object.get("requests") else null) orelse return .{ .unpriced = 1 };
    for (reqs.items) |r| {
        if (r != .object) continue;
        const p = r.object.get("params") orelse continue;
        var w: std.Io.Writer.Allocating = .init(std.heap.c_allocator);
        defer w.deinit();
        std.json.Stringify.value(p, .{}, &w.writer) catch continue;
        boundOfRequest(table, "anthropic", .anthropic, w.written(), day, bytes_per_token, &out);
    }
    return out;
}

/// What a batch's results cost: each line's own counts, at its model's batch price. Lines that
/// failed, were cancelled or expired are not billed.
pub const Settled = struct { cost: f64 = 0, lines: u32 = 0, unpriced: u32 = 0 };

pub fn costOfResults(gpa: Allocator, table: *prices.Table, provider: []const u8, jsonl: []const u8, day: []const u8) Settled {
    var out: Settled = .{};
    var it = std.mem.splitScalar(u8, jsonl, '\n');
    while (it.next()) |line| {
        if (std.mem.trim(u8, line, " \r").len == 0) continue;
        var parsed = std.json.parseFromSlice(std.json.Value, gpa, line, .{}) catch continue;
        defer parsed.deinit();
        if (parsed.value != .object) continue;
        const o = parsed.value.object;
        var format: usage.Format = .anthropic;
        var body: ?std.json.Value = null;
        if (eq(provider, "anthropic")) {
            const res = usage.obj(o.get("result")) orelse continue;
            if (!eq(usage.str(res.get("type")) orelse "", "succeeded")) continue;
            body = res.get("message");
        } else {
            const resp = usage.obj(o.get("response")) orelse continue;
            body = resp.get("body");
            const b = usage.obj(body) orelse continue;
            format = if (eq(usage.str(b.get("object")) orelse "", "response")) .responses else .chat;
        }
        const b = body orelse continue;
        var w: std.Io.Writer.Allocating = .init(gpa);
        defer w.deinit();
        std.json.Stringify.value(b, .{}, &w.writer) catch continue;
        var m = usage.Meter.init(gpa, format);
        defer m.deinit();
        m.onBody(w.written()) catch continue;
        out.lines += 1;
        const model = m.model orelse {
            out.unpriced += 1;
            continue;
        };
        const r = table.find(provider, model, "batch", day) orelse {
            out.unpriced += 1;
            continue;
        };
        out.cost += prices.cost(r, m.billed());
    }
    return out;
}

/// A duration written as Google writes it, "3600s", in seconds. Gemini's default is an hour.
pub fn ttlSeconds(v: ?[]const u8) f64 {
    const s = v orelse return 3600;
    return std.fmt.parseFloat(f64, std.mem.trimRight(u8, s, "s")) catch 3600;
}

fn eq(a: []const u8, b: []const u8) bool {
    return std.mem.eql(u8, a, b);
}

test "paths" {
    try std.testing.expectEqual(Kind.openai_batch_get, classify(.openai, "GET", "/v1/batches/batch_1"));
    try std.testing.expectEqual(Kind.response_get, classify(.openai, "GET", "/v1/responses/resp_1"));
    try std.testing.expectEqual(Kind.none, classify(.openai, "GET", "/v1/responses/resp_1/input_items"));
    try std.testing.expectEqual(Kind.response_cancel, classify(.openai, "POST", "/v1/responses/resp_1/cancel"));
    try std.testing.expectEqual(Kind.anthropic_batch_results, classify(.anthropic, "GET", "/v1/messages/batches/m1/results"));
    try std.testing.expectEqual(Kind.gemini_cache_delete, classify(.google, "DELETE", "/v1beta/cachedContents/abc"));
    try std.testing.expectEqualStrings("resp_1", pathPart("/v1/responses/resp_1?x=1", 2).?);
}

test "the batch file of a multipart upload" {
    const body = "--B\r\ncontent-disposition: form-data; name=\"purpose\"\r\n\r\nbatch\r\n--B\r\ncontent-disposition: form-data; name=\"file\"; filename=\"b.jsonl\"\r\n\r\n{\"a\":1}\n\r\n--B--\r\n";
    try std.testing.expectEqualStrings("{\"a\":1}\n", batchFile(body, "multipart/form-data; boundary=B").?);
    try std.testing.expect(batchFile("--B\r\ncontent-disposition: form-data; name=\"purpose\"\r\n\r\nfine-tune\r\n--B--", "multipart/form-data; boundary=B") == null);
}
