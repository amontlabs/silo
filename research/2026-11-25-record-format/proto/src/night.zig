//! A night of lines, to measure the record's size. The shapes are those of `../../format.md`; the
//! counts come from a load model (below) and are stated with the results.
const std = @import("std");
const Writer = @import("writer.zig").Writer;
const Body = @import("line.zig").Body;
const Redactor = @import("redact.zig").Redactor;

pub const Load = struct {
    name: []const u8,
    agents: u32,
    hours: u32,
    /// Seconds between two turns of one agent.
    turn_every_s: u32,
    /// Processes each tool call starts: the one asked for and the ones the agent program runs by
    /// itself (#371: Claude Code runs git about six times a turn).
    processes_per_turn: u32,
    /// Writes the probe reports per turn.
    file_reports_per_turn: u32,
    /// File facts from checkpoint comparisons over the night.
    file_facts: u32,
    checkpoint_every_s: u32,
    /// Turns after which a conversation is compacted and starts again.
    conversation_turns: u32,
    /// Share of requests, in percent, where the agent program rewrote an earlier message (#371
    /// finding 1), so the request is not the one before plus new messages.
    rewrite_pct: u32,
};

/// #370's morning: three agents for 8 hours, 96 checkpoints, 7,412 files changed.
pub const heavy = Load{
    .name = "heavy",
    .agents = 3,
    .hours = 8,
    .turn_every_s = 20,
    .processes_per_turn = 7,
    .file_reports_per_turn = 5,
    .file_facts = 7412,
    .checkpoint_every_s = 300,
    .conversation_turns = 150,
    .rewrite_pct = 5,
};

/// One agent, one hour, a quiet task: a floor.
pub const light = Load{
    .name = "light",
    .agents = 1,
    .hours = 1,
    .turn_every_s = 30,
    .processes_per_turn = 4,
    .file_reports_per_turn = 3,
    .file_facts = 120,
    .checkpoint_every_s = 300,
    .conversation_turns = 150,
    .rewrite_pct = 5,
};

pub const SentForm = enum {
    /// `sent` lists the hash of every message the request sent (`store-messages-once` as taken).
    full,
    /// `sent` names the model-call line whose request this one extends, and lists only the new
    /// messages; the full list when the program rewrote an earlier message.
    delta,
};

pub const Stats = struct {
    lines: u64 = 0,
    by_kind: [9]u64 = [_]u64{0} ** 9,
    bytes_by_kind: [9]u64 = [_]u64{0} ** 9,
    blob_bytes: u64 = 0,
    blobs: u64 = 0,
    /// What resending the whole conversation each time would have stored, for comparison.
    raw_request_bytes: u64 = 0,
    full_lists: u64 = 0,
};

const words = [_][]const u8{ "the", "test", "auth", "session", "store", "file", "error", "return", "const", "function", "await", "import", "from", "export", "if", "else", "null", "user", "request", "response", "token", "expires", "cookie", "handler", "router", "middleware", "config", "database", "query", "result", "assert", "expect", "describe", "it", "should", "pass", "fail", "build", "npm", "run", "src", "lib", "index", "ts", "js", "json", "path", "write", "read", "line", "column" };

fn text(rng: std.Random, gpa: std.mem.Allocator, len: usize) ![]u8 {
    var out: std.ArrayList(u8) = .empty;
    while (out.items.len < len) {
        try out.appendSlice(gpa, words[rng.uintLessThan(usize, words.len)]);
        try out.append(gpa, if (rng.uintLessThan(u8, 12) == 0) '\n' else ' ');
        if (rng.uintLessThan(u8, 9) == 0) {
            var nb: [12]u8 = undefined;
            try out.appendSlice(gpa, std.fmt.bufPrint(&nb, "{d} ", .{rng.uintLessThan(u32, 100000)}) catch unreachable);
        }
    }
    return out.toOwnedSlice(gpa);
}

const dirs = [_][]const u8{ "/Users/you/work/app/src/auth", "/Users/you/work/app/src/routes", "/Users/you/work/app/test", "/Users/you/work/app/node_modules/.cache", "/Users/you/work/app/dist", "/Users/you/.claude/projects/-Users-you-work-app", "/Users/you/work/client-b/src" };
const programs = [_][]const u8{ "/usr/bin/git status --porcelain", "/usr/bin/git diff --stat", "/usr/bin/git rev-parse HEAD", "/usr/local/bin/node node_modules/.bin/vitest run test/auth", "/usr/local/bin/npm test", "/bin/zsh -c rg -n session src", "/usr/bin/git log --oneline -5" };

fn path(rng: std.Random, buf: []u8) []const u8 {
    return std.fmt.bufPrint(buf, "{s}/{s}-{d}.{s}", .{ dirs[rng.uintLessThan(usize, dirs.len)], words[rng.uintLessThan(usize, words.len)], rng.uintLessThan(u32, 400), if (rng.boolean()) "ts" else "json" }) catch unreachable;
}

const kind_index = std.StaticStringMap(usize).initComptime(.{ .{ "control", 0 }, .{ "process", 1 }, .{ "file", 2 }, .{ "connection", 3 }, .{ "model-call", 4 }, .{ "screen", 5 }, .{ "meter", 6 }, .{ "gap", 7 }, .{ "silence", 8 } });

const Ctx = struct {
    gpa: std.mem.Allocator,
    w: *Writer,
    st: *Stats,
    t0: i128,
    counters: [4]u64 = .{ 0, 0, 0, 0 },

    fn put(c: *Ctx, t_s: f64, src: usize, is: []const u8, kind: []const u8, body: []const u8) !void {
        const ids = [_][]const u8{ "engine", "probe", "boundary", "disk" };
        const counts = [_][]const u8{ "h:5b1e44c0", "k:91", "b:3", "k:12" };
        c.counters[src] += 1;
        const before = c.w.pending.items.len + c.w.seg_size;
        _ = try c.w.append(.{ .time_ns = c.t0 + @as(i128, @intFromFloat(t_s * 1e9)), .source = ids[src], .count = counts[src], .n = c.counters[src], .is = is, .kind = kind, .body = body });
        const after = c.w.pending.items.len + c.w.seg_size;
        const k = kind_index.get(kind).?;
        c.st.lines += 1;
        c.st.by_kind[k] += 1;
        c.st.bytes_by_kind[k] += if (after >= before) after - before else after - @import("format.zig").header_size;
        if (c.w.pending.items.len > 1 << 20) try c.w.flush();
    }
};

const Conversation = struct {
    msgs: std.ArrayList([71]u8) = .empty,
    sizes: std.ArrayList(u64) = .empty,
    last_line: u64 = 0,
    turns: u32 = 0,
};

pub fn run(gpa: std.mem.Allocator, w: *Writer, load: Load, form: SentForm, seed: u64) !Stats {
    var prng = std.Random.DefaultPrng.init(seed);
    const rng = prng.random();
    var st = Stats{};
    var c = Ctx{ .gpa = gpa, .w = w, .st = &st, .t0 = 1795564800 * std.time.ns_per_s };
    const secs = load.hours * 3600;
    const convs = try gpa.alloc(Conversation, load.agents);
    defer {
        for (convs) |*cv| {
            cv.msgs.deinit(gpa);
            cv.sizes.deinit(gpa);
        }
        gpa.free(convs);
    }
    for (convs) |*cv| cv.* = .{};
    var pid: i64 = 4000;
    var pb: [256]u8 = undefined;
    const file_facts_per_ckpt = load.file_facts / @max(1, secs / load.checkpoint_every_s);
    var ckpt: i64 = 400;

    var t: u32 = 0;
    while (t < secs) : (t += 1) {
        const tf: f64 = @floatFromInt(t);
        // Each agent's turn.
        for (convs, 0..) |*cv, a| {
            if ((t + @as(u32, @intCast(a)) * 7) % load.turn_every_s != 0) continue;
            if (cv.turns == load.conversation_turns) {
                cv.msgs.clearRetainingCapacity();
                cv.sizes.clearRetainingCapacity();
                cv.turns = 0;
            }
            cv.turns += 1;
            // New messages of this turn: the tool result, then the model's reply with its tool call.
            const result_len = 300 + rng.uintLessThan(usize, 4000);
            const reply_len = 100 + rng.uintLessThan(usize, 1200);
            const rewritten = cv.msgs.items.len > 2 and rng.uintLessThan(u32, 100) < load.rewrite_pct;
            if (rewritten) {
                // The program changed an earlier message: a new hash for it.
                const i = rng.uintLessThan(usize, cv.msgs.items.len);
                const m = try text(rng, gpa, cv.sizes.items[i]);
                defer gpa.free(m);
                cv.msgs.items[i] = try w.putBlob(m);
                st.blob_bytes += m.len;
                st.blobs += 1;
            }
            const res = try text(rng, gpa, result_len);
            defer gpa.free(res);
            const reply = try text(rng, gpa, reply_len);
            defer gpa.free(reply);
            const input = programs[rng.uintLessThan(usize, programs.len)];
            const added_from = cv.msgs.items.len;
            try cv.msgs.append(gpa, try w.putBlob(res));
            try cv.sizes.append(gpa, res.len);
            const reply_ref = try w.putBlob(reply);
            st.blob_bytes += res.len + reply.len;
            st.blobs += 2;
            for (cv.sizes.items) |s| st.raw_request_bytes += s;

            var b = Body.init(gpa, w.opts.redactor);
            defer b.deinit();
            try b.str("provider", "anthropic");
            try b.str("login", "subscription");
            try b.str("model_asked", "claude-opus-5-5");
            try b.str("model", "claude-opus-5-5");
            try b.object("tokens");
            try b.int("in", @intCast(cv.sizes.items.len * 900 + rng.uintLessThan(u32, 500)));
            try b.int("out", @intCast(reply_len / 4));
            try b.int("cache_read", @intCast(cv.sizes.items.len * 850));
            try b.int("cache_write", @intCast(result_len / 4));
            try b.end();
            try b.int("cost_micro_usd", @intCast(rng.uintLessThan(u32, 90000)));
            var cb: [32]u8 = undefined;
            try b.str("conversation", std.fmt.bufPrint(&cb, "cv-{d}-{d}", .{ a, cv.msgs.items.len / 300 }) catch unreachable);
            try b.int("turn", cv.turns);
            const full = form == .full or rewritten or cv.last_line == 0;
            if (full) {
                st.full_lists += 1;
                try b.array("sent");
                for (cv.msgs.items) |h| try b.itemStr(&h);
                try b.endArray();
            } else {
                try b.object("sent");
                try b.int("after", @intCast(cv.last_line));
                try b.array("added");
                for (cv.msgs.items[added_from..]) |h| try b.itemStr(&h);
                try b.endArray();
                try b.end();
            }
            try b.str("reply", &reply_ref);
            try b.array("tool_calls");
            try b.itemObject();
            var tb: [32]u8 = undefined;
            try b.str("id", std.fmt.bufPrint(&tb, "toolu_{x}", .{rng.int(u64)}) catch unreachable);
            try b.str("name", "Bash");
            try b.str("command", input);
            try b.end();
            try b.endArray();
            cv.last_line = w.next_seq;
            try c.put(tf, 2, "fact", "model-call", b.buf.items);
            // The reply is sent back as a message of the next request.
            try cv.msgs.append(gpa, reply_ref);
            try cv.sizes.append(gpa, reply.len);

            // What the tool call left: processes and writes, as the probe reports them.
            var p: u32 = 0;
            while (p < load.processes_per_turn) : (p += 1) {
                pid += 1;
                var r = Body.init(gpa, w.opts.redactor);
                defer r.deinit();
                try r.str("event", "started");
                try r.int("pid", pid);
                try r.int("parent", pid - 1 - @as(i64, p));
                try r.int("uid", 501);
                try r.str("command", if (p == 0) input else programs[rng.uintLessThan(usize, 3)]);
                try c.put(tf + 0.01, 1, "report", "process", r.buf.items);
                var e = Body.init(gpa, w.opts.redactor);
                defer e.deinit();
                try e.str("event", "exited");
                try e.int("pid", pid);
                try e.int("exit", if (rng.uintLessThan(u8, 10) == 0) 1 else 0);
                try e.int("cpu_ms", rng.uintLessThan(u32, 4000));
                try e.int("rss_max_kb", 20000 + rng.uintLessThan(u32, 300000));
                try c.put(tf + 0.5, 1, "report", "process", e.buf.items);
            }
            var f: u32 = 0;
            while (f < load.file_reports_per_turn) : (f += 1) {
                var r = Body.init(gpa, w.opts.redactor);
                defer r.deinit();
                try r.str("op", "wrote");
                try r.int("pid", pid);
                try r.str("path", path(rng, &pb));
                try c.put(tf + 0.3, 1, "report", "file", r.buf.items);
            }
        }
        // A checkpoint, and the comparison with the one before.
        if (t % load.checkpoint_every_s == 0) {
            ckpt += 1;
            var b = Body.init(gpa, w.opts.redactor);
            defer b.deinit();
            try b.author("silo", null);
            try b.str("what", "checkpoint-taken");
            try b.int("checkpoint", ckpt);
            try b.str("reason", "interval");
            try b.int("made_ms", 1500 + rng.uintLessThan(u32, 800));
            try c.put(tf, 0, "fact", "control", b.buf.items);
            var k: u32 = 0;
            while (k < file_facts_per_ckpt) : (k += 1) {
                var ff = Body.init(gpa, w.opts.redactor);
                defer ff.deinit();
                try ff.array("between");
                try ff.itemInt(ckpt - 1);
                try ff.itemInt(ckpt);
                try ff.endArray();
                try ff.str("change", if (rng.uintLessThan(u8, 3) == 0) "created" else "changed");
                try ff.str("type", "file");
                try ff.str("path", path(rng, &pb));
                try ff.int("object", 7700000 + rng.uintLessThan(u32, 900000));
                try ff.array("size");
                try ff.itemInt(rng.uintLessThan(u32, 9000));
                try ff.itemInt(rng.uintLessThan(u32, 9000));
                try ff.endArray();
                try ff.array("what");
                try ff.itemStr("content");
                try ff.endArray();
                try c.put(tf + 20, 3, "fact", "file", ff.buf.items);
            }
            var cl = Body.init(gpa, w.opts.redactor);
            defer cl.deinit();
            try cl.array("between");
            try cl.itemInt(ckpt - 1);
            try cl.itemInt(ckpt);
            try cl.endArray();
            try cl.str("change", "compared");
            try cl.int("facts", file_facts_per_ckpt);
            try c.put(tf + 21, 3, "fact", "file", cl.buf.items);
        }
        // A sample of the computer's use every minute.
        if (t % 60 == 0) {
            var b = Body.init(gpa, w.opts.redactor);
            defer b.deinit();
            try b.int("disk_bytes", 41000000000 + @as(i64, rng.uintLessThan(u32, 900000000)));
            try b.int("memory_bytes", 9000000000 + @as(i64, rng.uintLessThan(u32, 3000000000)));
            try b.int("cpu_permille", rng.uintLessThan(u32, 4000));
            try c.put(tf, 0, "fact", "meter", b.buf.items);
        }
        // Other hosts: a connection now and then.
        if (t % 97 == 0) {
            var b = Body.init(gpa, w.opts.redactor);
            defer b.deinit();
            try b.str("host", "registry.npmjs.org");
            try b.int("port", 443);
            try b.str("decision", "allowed");
            try b.str("decided_by", "rule:hosts-for-builds@3");
            try b.str("method", "GET");
            try b.str("path", "/vitest/-/vitest-4.1.2.tgz");
            try b.int("status", 200);
            try b.int("bytes_out", 1200);
            try b.int("bytes_in", 380000 + rng.uintLessThan(u32, 100000));
            try c.put(tf, 2, "fact", "connection", b.buf.items);
        }
    }
    try w.flush();
    return st;
}
