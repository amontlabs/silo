//! Builds the keys of one line after its envelope. Every string goes through the redactor, so no
//! caller can put a secret's value in the record by forgetting to.
const std = @import("std");
const Redactor = @import("redact.zig").Redactor;

pub const kinds = [_][]const u8{ "control", "process", "file", "connection", "model-call", "screen", "meter", "gap", "silence" };
/// `who-made-each-change`, with `device` as #370 proposes for what the Mac does by itself.
pub const authors = [_][]const u8{ "you", "rule", "precaution", "extension", "silo", "device" };

pub const Body = struct {
    gpa: std.mem.Allocator,
    redactor: Redactor,
    buf: std.ArrayList(u8) = .empty,
    /// One flag per open object or array: whether a member was written in it yet.
    first: [16]bool = [_]bool{true} ** 16,
    depth: usize = 0,
    redacted: usize = 0,

    pub fn init(gpa: std.mem.Allocator, redactor: Redactor) Body {
        return .{ .gpa = gpa, .redactor = redactor };
    }

    pub fn deinit(b: *Body) void {
        b.buf.deinit(b.gpa);
    }

    fn sep(b: *Body) !void {
        if (!b.first[b.depth]) try b.buf.append(b.gpa, ',');
        b.first[b.depth] = false;
    }

    fn key(b: *Body, k: []const u8) !void {
        try b.sep();
        try b.buf.append(b.gpa, '"');
        try b.buf.appendSlice(b.gpa, k);
        try b.buf.appendSlice(b.gpa, "\":");
    }

    fn quoted(b: *Body, v: []const u8) !void {
        var clean: std.ArrayList(u8) = .empty;
        defer clean.deinit(b.gpa);
        b.redacted += try b.redactor.apply(b.gpa, &clean, v);
        try b.buf.append(b.gpa, '"');
        for (clean.items) |c| switch (c) {
            '"' => try b.buf.appendSlice(b.gpa, "\\\""),
            '\\' => try b.buf.appendSlice(b.gpa, "\\\\"),
            '\n' => try b.buf.appendSlice(b.gpa, "\\n"),
            '\r' => try b.buf.appendSlice(b.gpa, "\\r"),
            '\t' => try b.buf.appendSlice(b.gpa, "\\t"),
            0...8, 11, 12, 14...0x1f, 0x7f => {
                var e: [6]u8 = undefined;
                try b.buf.appendSlice(b.gpa, std.fmt.bufPrint(&e, "\\u{x:0>4}", .{c}) catch unreachable);
            },
            else => try b.buf.append(b.gpa, c),
        };
        try b.buf.append(b.gpa, '"');
    }

    pub fn str(b: *Body, k: []const u8, v: []const u8) !void {
        try b.key(k);
        try b.quoted(v);
    }

    pub fn int(b: *Body, k: []const u8, v: i64) !void {
        try b.key(k);
        var tmp: [24]u8 = undefined;
        try b.buf.appendSlice(b.gpa, std.fmt.bufPrint(&tmp, "{d}", .{v}) catch unreachable);
    }

    pub fn boolean(b: *Body, k: []const u8, v: bool) !void {
        try b.key(k);
        try b.buf.appendSlice(b.gpa, if (v) "true" else "false");
    }

    pub fn object(b: *Body, k: []const u8) !void {
        try b.key(k);
        try b.open('{');
    }

    pub fn array(b: *Body, k: []const u8) !void {
        try b.key(k);
        try b.open('[');
    }

    /// An object as an item of the array that is open.
    pub fn itemObject(b: *Body) !void {
        try b.sep();
        try b.open('{');
    }

    pub fn itemStr(b: *Body, v: []const u8) !void {
        try b.sep();
        try b.quoted(v);
    }

    pub fn itemInt(b: *Body, v: i64) !void {
        try b.sep();
        var tmp: [24]u8 = undefined;
        try b.buf.appendSlice(b.gpa, std.fmt.bufPrint(&tmp, "{d}", .{v}) catch unreachable);
    }

    fn open(b: *Body, c: u8) !void {
        try b.buf.append(b.gpa, c);
        b.depth += 1;
        b.first[b.depth] = true;
    }

    pub fn close(b: *Body, c: u8) !void {
        try b.buf.append(b.gpa, c);
        b.depth -= 1;
    }

    pub fn end(b: *Body) !void {
        try b.close('}');
    }

    pub fn endArray(b: *Body) !void {
        try b.close(']');
    }

    /// The author of a control fact, which `every-control-fact-has-its-author` asks of every one.
    pub fn author(b: *Body, by: []const u8, person: ?[]const u8) !void {
        try b.object("author");
        try b.str("by", by);
        if (person) |p| try b.str("person", p);
        try b.end();
    }
};

pub fn isKind(k: []const u8) bool {
    for (kinds) |x| if (std.mem.eql(u8, x, k)) return true;
    return false;
}

pub fn isAuthor(a: []const u8) bool {
    for (authors) |x| if (std.mem.eql(u8, x, a)) return true;
    return false;
}

test "strings are escaped and redacted" {
    const gpa = std.testing.allocator;
    var b = Body.init(gpa, try Redactor.init(&.{.{ .name = "k", .value = "SECRETVALUE1" }}));
    defer b.deinit();
    try b.str("cmd", "echo \"SECRETVALUE1\"\n");
    try b.array("argv");
    try b.itemStr("a\tb");
    try b.endArray();
    try std.testing.expectEqualStrings("\"cmd\":\"echo \\\"[secret:k]\\\"\\n\",\"argv\":[\"a\\tb\"]", b.buf.items);
    try std.testing.expectEqual(@as(usize, 1), b.redacted);
}
