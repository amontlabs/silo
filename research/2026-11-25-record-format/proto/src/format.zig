//! The bytes of a segment of the record: its header and its frames. See `../../format.md`.
const std = @import("std");

pub const Sha256 = std.crypto.hash.sha2.Sha256;
pub const Crc32c = std.hash.crc.Crc32Iscsi;

pub const magic = "SILOREC\x00";
pub const format_version: u16 = 1;
pub const header_size = 128;
pub const frame_header_size = 8;
/// The largest line a frame may hold. Bulky content goes to a blob, so a line stays small.
pub const max_line = 256 * 1024;
pub const zero_hash = [_]u8{0} ** 32;

pub const Header = struct {
    compression: u8 = 0,
    computer: u64,
    segment: u32,
    first_seq: u64,
    prev: [32]u8,

    pub fn encode(h: Header) [header_size]u8 {
        var b = [_]u8{0} ** header_size;
        @memcpy(b[0..8], magic);
        std.mem.writeInt(u16, b[8..10], format_version, .little);
        b[10] = h.compression;
        std.mem.writeInt(u64, b[16..24], h.computer, .little);
        std.mem.writeInt(u32, b[24..28], h.segment, .little);
        std.mem.writeInt(u64, b[32..40], h.first_seq, .little);
        @memcpy(b[40..72], &h.prev);
        Sha256.hash(b[0..96], b[96..128], .{});
        return b;
    }

    pub const DecodeError = error{ BadMagic, BadVersion, BadChecksum, BadReserved };

    pub fn decode(b: *const [header_size]u8) DecodeError!Header {
        if (!std.mem.eql(u8, b[0..8], magic)) return error.BadMagic;
        if (std.mem.readInt(u16, b[8..10], .little) != format_version) return error.BadVersion;
        var sum: [32]u8 = undefined;
        Sha256.hash(b[0..96], &sum, .{});
        if (!std.mem.eql(u8, &sum, b[96..128])) return error.BadChecksum;
        for (b[11..16]) |x| if (x != 0) return error.BadReserved;
        for (b[28..32]) |x| if (x != 0) return error.BadReserved;
        for (b[72..96]) |x| if (x != 0) return error.BadReserved;
        return .{
            .compression = b[10],
            .computer = std.mem.readInt(u64, b[16..24], .little),
            .segment = std.mem.readInt(u32, b[24..28], .little),
            .first_seq = std.mem.readInt(u64, b[32..40], .little),
            .prev = b[40..72].*,
        };
    }
};

pub fn frameHeader(payload: []const u8) [frame_header_size]u8 {
    var b: [frame_header_size]u8 = undefined;
    std.mem.writeInt(u32, b[0..4], @intCast(payload.len), .little);
    std.mem.writeInt(u32, b[4..8], Crc32c.hash(payload), .little);
    return b;
}

pub fn lineHash(line: []const u8) [32]u8 {
    var h: [32]u8 = undefined;
    Sha256.hash(line, &h, .{});
    return h;
}

pub fn hex(h: [32]u8) [64]u8 {
    return std.fmt.bytesToHex(h, .lower);
}

pub fn segmentName(buf: []u8, n: u32) []const u8 {
    return std.fmt.bufPrint(buf, "{d:0>6}.silorec", .{n}) catch unreachable;
}

/// Parses `NNNNNN.silorec`; anything else is not a segment.
pub fn parseSegmentName(name: []const u8) ?u32 {
    if (name.len != 14 or !std.mem.endsWith(u8, name, ".silorec")) return null;
    return std.fmt.parseInt(u32, name[0..6], 10) catch null;
}

/// The id a computer's name stands for in a segment header: `engine` is 0, `cN` is N.
pub fn computerId(name: []const u8) ?u64 {
    if (std.mem.eql(u8, name, "engine")) return 0;
    if (name.len < 2 or name[0] != 'c') return null;
    const n = std.fmt.parseInt(u64, name[1..], 10) catch return null;
    if (n == 0) return null;
    return n;
}

/// UTC, in nanoseconds: 2026-11-25T02:41:07.123456789Z
pub fn formatTime(buf: *[30]u8, ns: i128) []const u8 {
    const secs: u64 = @intCast(@divFloor(ns, std.time.ns_per_s));
    const frac: u64 = @intCast(@mod(ns, std.time.ns_per_s));
    const es = std.time.epoch.EpochSeconds{ .secs = secs };
    const yd = es.getEpochDay().calculateYearDay();
    const md = yd.calculateMonthDay();
    const ds = es.getDaySeconds();
    return std.fmt.bufPrint(buf, "{d:0>4}-{d:0>2}-{d:0>2}T{d:0>2}:{d:0>2}:{d:0>2}.{d:0>9}Z", .{
        yd.year, md.month.numeric(), md.day_index + 1, ds.getHoursIntoDay(), ds.getMinutesIntoHour(), ds.getSecondsIntoMinute(), frac,
    }) catch unreachable;
}

test "header round trip and checksum" {
    const h = Header{ .computer = 1, .segment = 7, .first_seq = 1042, .prev = [_]u8{0xab} ** 32 };
    var b = h.encode();
    const d = try Header.decode(&b);
    try std.testing.expectEqual(@as(u64, 1042), d.first_seq);
    b[24] ^= 1;
    try std.testing.expectError(error.BadChecksum, Header.decode(&b));
}

test "time" {
    var buf: [30]u8 = undefined;
    try std.testing.expectEqualStrings("2026-11-25T02:41:07.123456789Z", formatTime(&buf, 1795574467123456789));
}

/// Walks the frames of one segment's bytes, after its header.
pub const Frames = struct {
    bytes: []const u8,
    off: usize = header_size,

    pub const Bad = enum {
        /// Fewer than 8 bytes left: a frame header cut short.
        short_header,
        /// A length over `max_line`: never something the recorder writes.
        too_long,
        /// A length that runs past the end of the file: a frame cut short.
        past_end,
        /// The payload does not match its CRC.
        crc,

        pub fn text(b: Bad) []const u8 {
            return switch (b) {
                .short_header => "a frame header is cut short",
                .too_long => "a frame says it is longer than any line may be",
                .past_end => "a frame runs past the end of the segment",
                .crc => "a line does not match its CRC",
            };
        }
    };

    pub const Item = union(enum) {
        line: struct { off: usize, payload: []const u8 },
        end,
        /// `torn` is true when the bad frame is the last thing in the segment and could be a
        /// write the recorder did not finish: the bytes from it to the end are one frame cut short,
        /// or one frame whose end is the end of the file.
        bad: struct { off: usize, why: Bad, torn: bool },
    };

    pub fn next(f: *Frames) Item {
        const b = f.bytes;
        if (f.off == b.len) return .end;
        const off = f.off;
        // A segment shorter than its header: the fuzzer found the walk overflowing here.
        if (off > b.len) return .{ .bad = .{ .off = b.len, .why = .short_header, .torn = false } };
        if (b.len - off < frame_header_size) return .{ .bad = .{ .off = off, .why = .short_header, .torn = true } };
        const len = std.mem.readInt(u32, b[off..][0..4], .little);
        if (len > max_line) return .{ .bad = .{ .off = off, .why = .too_long, .torn = false } };
        const end = off + frame_header_size + len;
        if (end > b.len) return .{ .bad = .{ .off = off, .why = .past_end, .torn = true } };
        const payload = b[off + frame_header_size .. end];
        if (std.mem.readInt(u32, b[off + 4 ..][0..4], .little) != Crc32c.hash(payload))
            return .{ .bad = .{ .off = off, .why = .crc, .torn = end == b.len } };
        f.off = end;
        return .{ .line = .{ .off = off, .payload = payload } };
    }
};
