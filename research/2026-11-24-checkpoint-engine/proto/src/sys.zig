//! The few system calls the prototype needs, on Linux and on macOS.
//! Linux: FICLONE, FICLONERANGE, FIEMAP, Btrfs snapshots. macOS: clonefile(2) and F_FULLFSYNC.
const std = @import("std");
const builtin = @import("builtin");
const posix = std.posix;

pub const is_linux = builtin.os.tag == .linux;
pub const is_mac = builtin.os.tag.isDarwin();

pub fn now() u64 {
    const ts = posix.clock_gettime(.MONOTONIC) catch unreachable;
    return @as(u64, @intCast(ts.sec)) * std.time.ns_per_s + @as(u64, @intCast(ts.nsec));
}

pub fn secs(ns: u64) f64 {
    return @as(f64, @floatFromInt(ns)) / 1e9;
}

pub fn sleepNs(ns: u64) void {
    std.Thread.sleep(ns);
}

/// Opens a disk image for the writer: uncached on both systems (O_DIRECT on Linux, F_NOCACHE on
/// macOS), as a virtual machine's disk is attached in the uncached mode.
pub fn openImage(path: []const u8, create: bool) !posix.fd_t {
    if (is_linux) {
        return posix.open(path, .{ .ACCMODE = .RDWR, .DIRECT = true, .CREAT = create }, 0o644);
    } else {
        const fd = try posix.open(path, .{ .ACCMODE = .RDWR, .CREAT = create }, 0o644);
        _ = posix.system.fcntl(fd, 48, @as(c_int, 1)); // F_NOCACHE
        return fd;
    }
}

pub fn openRead(path: []const u8) !posix.fd_t {
    return posix.open(path, .{ .ACCMODE = .RDONLY }, 0);
}

/// Durable flush: F_FULLFSYNC on macOS (Zig's fsync is not durable there, #352 file 02),
/// fsync on Linux.
pub fn fullSync(fd: posix.fd_t) !void {
    if (is_mac) {
        const rc = posix.system.fcntl(fd, 51, @as(c_int, 0)); // F_FULLFSYNC
        if (rc != 0) return error.FullFsyncFailed;
    } else {
        try posix.fsync(fd);
    }
}

pub fn syncDir(path: []const u8) !void {
    const fd = try posix.open(path, .{ .ACCMODE = .RDONLY, .DIRECTORY = true }, 0);
    defer posix.close(fd);
    try fullSync(fd);
}

extern "c" fn clonefile(src: [*:0]const u8, dst: [*:0]const u8, flags: u32) c_int;

/// A whole-file clone that shares every block: FICLONE on Linux, clonefile(2) on macOS.
/// dst must not exist.
pub fn cloneFile(src: [:0]const u8, dst: [:0]const u8) !void {
    if (is_mac) {
        if (clonefile(src.ptr, dst.ptr, 0) != 0) return error.CloneFailed;
        return;
    }
    const s = try posix.open(src, .{ .ACCMODE = .RDONLY }, 0);
    defer posix.close(s);
    const d = try posix.open(dst, .{ .ACCMODE = .WRONLY, .CREAT = true, .EXCL = true }, 0o644);
    defer posix.close(d);
    const FICLONE: u32 = 0x40049409;
    const rc = std.os.linux.ioctl(d, FICLONE, @intCast(s));
    if (posix.errno(rc) != .SUCCESS) {
        std.debug.print("FICLONE: {s}\n", .{@tagName(posix.errno(rc))});
        return error.CloneFailed;
    }
}

const FileCloneRange = extern struct { src_fd: i64, src_offset: u64, src_length: u64, dest_offset: u64 };

/// Linux only: clone one range of src into dst at the same offset. APFS has no public equivalent.
pub fn cloneRange(src: posix.fd_t, dst: posix.fd_t, off: u64, len: u64) !void {
    if (!is_linux) return error.Unsupported;
    var a = FileCloneRange{ .src_fd = src, .src_offset = off, .src_length = len, .dest_offset = off };
    const FICLONERANGE: u32 = 0x4020940D;
    const rc = std.os.linux.ioctl(dst, FICLONERANGE, @intFromPtr(&a));
    if (posix.errno(rc) != .SUCCESS) {
        std.debug.print("FICLONERANGE {d}+{d}: {s}\n", .{ off, len, @tagName(posix.errno(rc)) });
        return error.CloneFailed;
    }
}

pub const Extent = struct { logical: u64, physical: u64, len: u64, shared: bool };

const FiemapHead = extern struct {
    start: u64,
    length: u64,
    flags: u32,
    mapped: u32,
    count: u32,
    reserved: u32,
};
const FiemapExtent = extern struct {
    logical: u64,
    physical: u64,
    length: u64,
    r1: u64,
    r2: u64,
    flags: u32,
    r3: u32,
    r4: u32,
    r5: u32,
};

/// Linux only: the block map of a file (FIEMAP), in order. Appends to `out`.
pub fn extents(gpa: std.mem.Allocator, fd: posix.fd_t, start: u64, length: u64, out: *std.ArrayList(Extent)) !void {
    if (!is_linux) return error.Unsupported;
    const N = 1024;
    const buf = try gpa.alignedAlloc(u8, .@"8", @sizeOf(FiemapHead) + N * @sizeOf(FiemapExtent));
    defer gpa.free(buf);
    var pos = start;
    const end = start +| length;
    while (pos < end) {
        const h: *FiemapHead = @ptrCast(buf.ptr);
        h.* = .{ .start = pos, .length = end - pos, .flags = 0, .mapped = 0, .count = N, .reserved = 0 };
        const FS_IOC_FIEMAP: u32 = 0xC020660B;
        const rc = std.os.linux.ioctl(fd, FS_IOC_FIEMAP, @intFromPtr(buf.ptr));
        if (posix.errno(rc) != .SUCCESS) return error.FiemapFailed;
        if (h.mapped == 0) break;
        const ex: [*]FiemapExtent = @ptrCast(@alignCast(buf.ptr + @sizeOf(FiemapHead)));
        var last = false;
        for (ex[0..h.mapped]) |e| {
            try out.append(gpa, .{ .logical = e.logical, .physical = e.physical, .len = e.length, .shared = e.flags & 0x2000 != 0 });
            pos = e.logical + e.length;
            if (e.flags & 1 != 0) last = true;
        }
        if (last) break;
    }
}

// Btrfs: a read-only snapshot of a subvolume, and subvolume removal.
const VolArgsV2 = extern struct {
    fd: i64,
    transid: u64,
    flags: u64,
    unused: [4]u64,
    name: [4040]u8,
};
const VolArgs = extern struct { fd: i64, name: [4088]u8 };

pub fn btrfsSnapshot(src_subvol: []const u8, dst_parent: []const u8, name: []const u8, readonly: bool) !void {
    if (!is_linux) return error.Unsupported;
    const s = try posix.open(src_subvol, .{ .ACCMODE = .RDONLY, .DIRECTORY = true }, 0);
    defer posix.close(s);
    const p = try posix.open(dst_parent, .{ .ACCMODE = .RDONLY, .DIRECTORY = true }, 0);
    defer posix.close(p);
    var a = std.mem.zeroes(VolArgsV2);
    a.fd = s;
    a.flags = if (readonly) 2 else 0;
    @memcpy(a.name[0..name.len], name);
    const SNAP_CREATE_V2: u32 = 0x50009417;
    const rc = std.os.linux.ioctl(p, SNAP_CREATE_V2, @intFromPtr(&a));
    if (posix.errno(rc) != .SUCCESS) {
        std.debug.print("snapshot: {s}\n", .{@tagName(posix.errno(rc))});
        return error.SnapshotFailed;
    }
}

pub fn btrfsCreateSubvol(parent: []const u8, name: []const u8) !void {
    const p = try posix.open(parent, .{ .ACCMODE = .RDONLY, .DIRECTORY = true }, 0);
    defer posix.close(p);
    var a = std.mem.zeroes(VolArgs);
    @memcpy(a.name[0..name.len], name);
    const SUBVOL_CREATE: u32 = 0x5000940E;
    const rc = std.os.linux.ioctl(p, SUBVOL_CREATE, @intFromPtr(&a));
    if (posix.errno(rc) != .SUCCESS) return error.SubvolFailed;
}

pub fn btrfsDestroy(parent: []const u8, name: []const u8) !void {
    const p = try posix.open(parent, .{ .ACCMODE = .RDONLY, .DIRECTORY = true }, 0);
    defer posix.close(p);
    var a = std.mem.zeroes(VolArgs);
    @memcpy(a.name[0..name.len], name);
    const SNAP_DESTROY: u32 = 0x5000940F;
    const rc = std.os.linux.ioctl(p, SNAP_DESTROY, @intFromPtr(&a));
    if (posix.errno(rc) != .SUCCESS) {
        std.debug.print("destroy: {s}\n", .{@tagName(posix.errno(rc))});
        return error.DestroyFailed;
    }
}
