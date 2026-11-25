//! The boundary as a source of the record (#370 `formats.md`, "How a source sends its lines"):
//! one connection to the recorder's socket, a 4-byte little-endian length then one JSON object
//! each way. Every line gets the next number `n`; the recorder acknowledges the highest number on
//! its disk, and the boundary keeps every line until then and sends them again after a reconnect.
//!
//! Two kinds of room, as `enforcing-and-recording` asks:
//! - a decision or a change to traffic (a connection allowed or refused, a call admitted or
//!   refused, a secret swapped in) waits for room and is never lost. When the room is full, the
//!   request that needs it waits, up to a limit, then is refused;
//! - any other line (bytes counted at a close, a call's final count, a lookup) goes into a buffer
//!   of fixed size, and when that is full the oldest such line is dropped. Its number is then
//!   missing, and the recorder writes a gap.
const std = @import("std");
const posix = std.posix;

pub const Class = enum { decision, observation };

const Line = struct { n: u64, class: Class, json: []u8 };

pub const Source = struct {
    gpa: std.mem.Allocator,
    path: ?[]const u8,
    /// The source's name, `boundary:c1`: one source per computer, so each computer's record
    /// numbers the boundary's lines without holes that belong to another computer.
    name: []const u8 = "boundary",
    decision_room: usize,
    observation_room: usize,
    mu: std.Thread.Mutex = .{},
    room: std.Thread.Condition = .{},
    wake: std.Thread.Condition = .{},
    pending: std.ArrayList(Line) = .empty,
    decisions: usize = 0,
    observations: usize = 0,
    next_n: u64 = 1,
    /// The highest number sent on the current connection.
    sent: u64 = 0,
    acked: u64 = 0,
    count_id: ?[]u8 = null,
    connected: bool = false,
    /// Observation lines dropped for lack of room, all told.
    dropped: u64 = 0,
    /// Decisions that waited for room, and those refused after waiting.
    waited: u64 = 0,
    refused: u64 = 0,

    /// Adds a line. The JSON is the line's own keys, without `n` (`"is":"fact","kind":...`). A
    /// decision waits for room up to `wait_ms`; error.NoRoom after that.
    pub fn add(self: *Source, class: Class, comptime fmt: []const u8, args: anytype, wait_ms: u64) !u64 {
        const body = try std.fmt.allocPrint(self.gpa, fmt, args);
        errdefer self.gpa.free(body);
        self.mu.lock();
        defer self.mu.unlock();
        switch (class) {
            .decision => if (self.decisions >= self.decision_room) {
                self.waited += 1;
                const deadline = std.time.nanoTimestamp() + @as(i128, wait_ms) * std.time.ns_per_ms;
                while (self.decisions >= self.decision_room) {
                    const left = deadline - std.time.nanoTimestamp();
                    if (left <= 0) {
                        self.refused += 1;
                        return error.NoRoom;
                    }
                    self.room.timedWait(&self.mu, @intCast(left)) catch {};
                }
            },
            .observation => if (self.observations >= self.observation_room) {
                for (self.pending.items, 0..) |l, i| if (l.class == .observation) {
                    self.gpa.free(l.json);
                    _ = self.pending.orderedRemove(i);
                    self.observations -= 1;
                    self.dropped += 1;
                    break;
                };
            },
        }
        const n = self.next_n;
        self.next_n += 1;
        const json = try std.fmt.allocPrint(self.gpa, "{{\"line\":{{\"n\":{d},{s}}}}}", .{ n, body });
        self.gpa.free(body);
        try self.pending.append(self.gpa, .{ .n = n, .class = class, .json = json });
        switch (class) {
            .decision => self.decisions += 1,
            .observation => self.observations += 1,
        }
        self.wake.signal();
        return n;
    }

    fn ackUpTo(self: *Source, n: u64) void {
        self.mu.lock();
        defer self.mu.unlock();
        if (n <= self.acked) return;
        self.acked = n;
        var k: usize = 0;
        while (k < self.pending.items.len and self.pending.items[k].n <= n) : (k += 1) {
            const l = self.pending.items[k];
            switch (l.class) {
                .decision => self.decisions -= 1,
                .observation => self.observations -= 1,
            }
            self.gpa.free(l.json);
        }
        if (k > 0) {
            std.mem.copyForwards(Line, self.pending.items, self.pending.items[k..]);
            self.pending.shrinkRetainingCapacity(self.pending.items.len - k);
            self.room.broadcast();
        }
    }

    /// Without a recorder, every line counts as acknowledged at once.
    pub fn start(self: *Source) !void {
        if (self.path == null) {
            (try std.Thread.spawn(.{}, discard, .{self})).detach();
            return;
        }
        (try std.Thread.spawn(.{}, run, .{self})).detach();
    }

    fn discard(self: *Source) void {
        while (true) {
            self.mu.lock();
            while (self.pending.items.len == 0) self.wake.wait(&self.mu);
            const last = self.pending.items[self.pending.items.len - 1].n;
            self.mu.unlock();
            self.ackUpTo(last);
        }
    }

    fn run(self: *Source) void {
        while (true) {
            self.session() catch {};
            self.mu.lock();
            self.connected = false;
            self.mu.unlock();
            std.Thread.sleep(200 * std.time.ns_per_ms);
        }
    }

    fn session(self: *Source) !void {
        const fd = try posix.socket(posix.AF.UNIX, posix.SOCK.STREAM | posix.SOCK.CLOEXEC, 0);
        defer posix.close(fd);
        var addr = try std.net.Address.initUnix(self.path.?);
        try posix.connect(fd, &addr.any, addr.getOsSockLen());
        {
            self.mu.lock();
            const hello = if (self.count_id) |cid|
                try std.fmt.allocPrint(self.gpa, "{{\"hello\":{{\"versions\":[1],\"source\":\"{s}\",\"count\":\"{s}\",\"n\":{d}}}}}", .{ self.name, cid, self.acked })
            else
                try std.fmt.allocPrint(self.gpa, "{{\"hello\":{{\"versions\":[1],\"source\":\"{s}\"}}}}", .{self.name});
            self.mu.unlock();
            defer self.gpa.free(hello);
            try sendFrame(fd, hello);
        }
        var buf: [4096]u8 = undefined;
        const first = try recvFrame(fd, &buf);
        const cid = jsonString(first, "\"count\":\"") orelse return error.NoCount;
        self.mu.lock();
        if (self.count_id == null or !std.mem.eql(u8, self.count_id.?, cid)) {
            if (self.count_id) |o| self.gpa.free(o);
            self.count_id = try self.gpa.dupe(u8, cid);
        }
        self.sent = self.acked;
        self.connected = true;
        self.mu.unlock();
        const reader = try std.Thread.spawn(.{}, readAcks, .{ self, fd });
        defer reader.join();
        defer posix.shutdown(fd, .both) catch {};
        while (true) {
            var batch: std.ArrayList(u8) = .empty;
            defer batch.deinit(self.gpa);
            {
                self.mu.lock();
                defer self.mu.unlock();
                while (true) {
                    var any = false;
                    for (self.pending.items) |l| if (l.n > self.sent) {
                        any = true;
                        break;
                    };
                    if (any) break;
                    self.wake.timedWait(&self.mu, 60 * std.time.ns_per_s) catch {
                        // Nothing to say for a minute: say the last number, as #370 asks.
                        const m = try std.fmt.allocPrint(self.gpa, "{{\"last\":{{\"n\":{d}}}}}", .{self.next_n - 1});
                        defer self.gpa.free(m);
                        try appendFrame(self.gpa, &batch, m);
                        break;
                    };
                    if (!self.connected) return error.Closed;
                }
                for (self.pending.items) |l| if (l.n > self.sent) {
                    try appendFrame(self.gpa, &batch, l.json);
                    self.sent = l.n;
                };
            }
            try writeAll(fd, batch.items);
        }
    }

    fn readAcks(self: *Source, fd: posix.fd_t) void {
        var buf: [4096]u8 = undefined;
        while (true) {
            const m = recvFrame(fd, &buf) catch break;
            if (jsonNumber(m, "\"ack\":")) |n| self.ackUpTo(n);
        }
        self.mu.lock();
        self.connected = false;
        self.wake.broadcast();
        self.mu.unlock();
    }
};

fn appendFrame(gpa: std.mem.Allocator, out: *std.ArrayList(u8), m: []const u8) !void {
    var len: [4]u8 = undefined;
    std.mem.writeInt(u32, &len, @intCast(m.len), .little);
    try out.appendSlice(gpa, &len);
    try out.appendSlice(gpa, m);
}

fn sendFrame(fd: posix.fd_t, m: []const u8) !void {
    var len: [4]u8 = undefined;
    std.mem.writeInt(u32, &len, @intCast(m.len), .little);
    try writeAll(fd, &len);
    try writeAll(fd, m);
}

fn writeAll(fd: posix.fd_t, b: []const u8) !void {
    var off: usize = 0;
    while (off < b.len) off += try posix.send(fd, b[off..], posix.MSG.NOSIGNAL);
}

fn recvFrame(fd: posix.fd_t, buf: []u8) ![]u8 {
    var len: [4]u8 = undefined;
    try readExact(fd, &len);
    const n = std.mem.readInt(u32, &len, .little);
    if (n > buf.len) return error.TooLarge;
    try readExact(fd, buf[0..n]);
    return buf[0..n];
}

fn readExact(fd: posix.fd_t, b: []u8) !void {
    var off: usize = 0;
    while (off < b.len) {
        const r = try posix.read(fd, b[off..]);
        if (r == 0) return error.Closed;
        off += r;
    }
}

fn jsonString(m: []const u8, key: []const u8) ?[]const u8 {
    const i = std.mem.indexOf(u8, m, key) orelse return null;
    const rest = m[i + key.len ..];
    const e = std.mem.indexOfScalar(u8, rest, '"') orelse return null;
    return rest[0..e];
}

fn jsonNumber(m: []const u8, key: []const u8) ?u64 {
    const i = std.mem.indexOf(u8, m, key) orelse return null;
    var rest = m[i + key.len ..];
    rest = std.mem.trimLeft(u8, rest, " ");
    var e: usize = 0;
    while (e < rest.len and std.ascii.isDigit(rest[e])) e += 1;
    return std.fmt.parseInt(u64, rest[0..e], 10) catch null;
}

const t = std.testing;

test "decisions wait, observations are dropped" {
    var s: Source = .{ .gpa = t.allocator, .path = null, .decision_room = 2, .observation_room = 2 };
    defer {
        for (s.pending.items) |l| t.allocator.free(l.json);
        s.pending.deinit(t.allocator);
    }
    _ = try s.add(.decision, "\"a\":{d}", .{1}, 0);
    _ = try s.add(.observation, "\"b\":{d}", .{2}, 0);
    _ = try s.add(.observation, "\"b\":{d}", .{3}, 0);
    _ = try s.add(.observation, "\"b\":{d}", .{4}, 0); // drops n=2
    try t.expectEqual(@as(u64, 1), s.dropped);
    _ = try s.add(.decision, "\"a\":{d}", .{5}, 0);
    try t.expectError(error.NoRoom, s.add(.decision, "\"a\":{d}", .{6}, 20));
    try t.expectEqual(@as(u64, 1), s.refused);
    s.ackUpTo(3);
    try t.expectEqual(@as(usize, 1), s.decisions);
    try t.expectEqualStrings("{\"line\":{\"n\":4,\"b\":4}}", s.pending.items[0].json);
    _ = try s.add(.decision, "\"a\":{d}", .{7}, 0);
}
