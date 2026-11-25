//! ckpt: a throwaway prototype of the checkpoint engine's core. Not Silo's code.
//!
//!   ckpt bench key=value...      checkpoints of a disk image a running computer writes (bench.zig)
//!   ckpt thin key=value...       thinning over weeks of simulated checkpoints (thin.zig)
//!   ckpt crash key=value...      the checkpoint catalog under kill -9 (catalog.zig)
//!   ckpt putback A B LIVE        put back what a run changed (putback.zig)
const std = @import("std");
const bench = @import("bench.zig");
const thin = @import("thin.zig");
const catalog = @import("catalog.zig");
const putback = @import("putback.zig");
const nbd = @import("nbd.zig");

/// Fills a struct of options from key=value arguments.
pub fn parse(comptime T: type, args: []const []const u8) !T {
    var o = T{};
    for (args) |a| {
        const eq = std.mem.indexOfScalar(u8, a, '=') orelse return error.BadArgument;
        const k = a[0..eq];
        const v = a[eq + 1 ..];
        var found = false;
        inline for (std.meta.fields(T)) |f| {
            if (std.mem.eql(u8, f.name, k)) {
                found = true;
                @field(o, f.name) = switch (@typeInfo(f.type)) {
                    .int => try std.fmt.parseInt(f.type, v, 10),
                    .float => try std.fmt.parseFloat(f.type, v),
                    .bool => std.mem.eql(u8, v, "1") or std.mem.eql(u8, v, "true"),
                    .@"enum" => std.meta.stringToEnum(f.type, v) orelse return error.BadArgument,
                    .pointer => v,
                    else => @compileError("option type"),
                };
            }
        }
        if (!found) {
            std.debug.print("unknown option {s}\n", .{k});
            return error.BadArgument;
        }
    }
    return o;
}

pub fn main() !void {
    const gpa = std.heap.page_allocator;
    const args = try std.process.argsAlloc(gpa);
    if (args.len < 2) {
        std.debug.print("usage: ckpt bench|thin|crash|putback ...\n", .{});
        return;
    }
    const cmd = args[1];
    const rest = args[2..];
    if (std.mem.eql(u8, cmd, "bench")) return bench.run(gpa, try parse(bench.Opts, rest));
    if (std.mem.eql(u8, cmd, "thin")) return thin.sim(gpa, try parse(thin.SimOpts, rest));
    if (std.mem.eql(u8, cmd, "crash")) return catalog.crashTest(gpa, try parse(catalog.CrashOpts, rest));
    if (std.mem.eql(u8, cmd, "crash-child")) return catalog.child(rest[0], try std.fmt.parseInt(u64, rest[1], 10));
    if (std.mem.eql(u8, cmd, "nbd-serve")) return nbd.serveMain(gpa, try parse(nbd.ServeOpts, rest));
    if (std.mem.eql(u8, cmd, "putback")) return putback.run(gpa, rest);
    std.debug.print("unknown command {s}\n", .{cmd});
}

test {
    _ = thin;
    _ = catalog;
}
