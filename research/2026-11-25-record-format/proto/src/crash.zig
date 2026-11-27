//! Crash points for the systematic crash test, as #365's catalog test does: the writer counts every
//! step that touches the disk, and a test run dies at the k-th one, for every k.
const std = @import("std");

/// The step to die at; 0 never dies.
pub var at: u64 = 0;
pub var count: u64 = 0;
pub var mutant: Mutant = .none;

/// Bugs planted in the writer, to show that the crash test catches them.
pub const Mutant = enum {
    none,
    /// Writes a line before the blob it points to is on disk.
    line_before_blob,
    /// Rewrites the head in place instead of a new file and a rename.
    head_in_place,
    /// Writes a line a source sends again after a restart a second time.
    no_dedupe,
    /// Cuts the last segment at the first bad frame, wherever it is.
    cut_any_bad_frame,
};

pub const exit_code = 77;

pub fn point() void {
    count += 1;
    if (count == at) std.posix.exit(exit_code);
}

/// Writes `bytes` at `off`; at the crash point, writes only the first half and dies, as a process
/// killed in the middle of a write leaves a prefix of it.
pub fn write(file: std.fs.File, bytes: []const u8, off: u64) !void {
    count += 1;
    if (count == at) {
        file.pwriteAll(bytes[0 .. bytes.len / 2], off) catch {};
        std.posix.exit(exit_code);
    }
    try file.pwriteAll(bytes, off);
}
